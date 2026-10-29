//! How long a Playback Source takes to start: from sending mpv `loadfile`
//! (or from the boundary where mpv moved to an appended entry) to
//! `file-loaded`, and to the first `playback-restart`, when audio actually
//! plays. One stderr line per start, naming only the source type and what
//! started it, never its URL. A load replaced by the next one before it
//! plays gets its own line, so no start goes unreported.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What started a load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadTrigger {
    /// A play command: a chosen source, or the Queue's next or previous.
    Play,
    /// The Queue advanced when the current Track ended.
    Queue,
    /// mpv moved on to the next Queue Track appended near the end.
    Gapless,
    /// Repeat Once played the finished Track again.
    Repeat,
    /// The saved session was restored on launch.
    Restore,
    /// A Radio Station reconnected after its stream dropped.
    Reconnect,
    /// The player was restarted after mpv exited.
    Recovery,
}

impl LoadTrigger {
    fn name(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Queue => "queue",
            Self::Gapless => "gapless",
            Self::Repeat => "repeat",
            Self::Restore => "restore",
            Self::Reconnect => "reconnect",
            Self::Recovery => "recovery",
        }
    }
}

/// What the event thread saw of a load.
pub(crate) enum LoadMark {
    /// mpv accepted the `loadfile` of this load revision; later events are
    /// about its file.
    Accepted(u64),
    FileLoaded,
    Playing,
    Failed,
}

struct Load {
    /// The load revision whose boundary marks events as this load's; `None`
    /// once every later event is.
    revision: Option<u64>,
    source_type: &'static str,
    trigger: LoadTrigger,
    sent_at: Instant,
    file_loaded: Option<Duration>,
}

impl Load {
    fn fields(&self) -> String {
        format!("type={} start={}", self.source_type, self.trigger.name())
    }
}

#[derive(Default)]
pub(crate) struct LoadTimer(Mutex<Option<Load>>);

impl LoadTimer {
    /// Starts timing a load just before its `loadfile` is sent. A
    /// `revision` waits for that load's boundary; `None` counts the events
    /// from now on. Returns the line for a load this one replaced before it
    /// played.
    pub(crate) fn start(
        &self,
        revision: Option<u64>,
        source_type: &'static str,
        trigger: LoadTrigger,
    ) -> Option<String> {
        let mut current = self.0.lock().ok()?;
        let replaced = current.replace(Load {
            revision,
            source_type,
            trigger,
            sent_at: Instant::now(),
            file_loaded: None,
        })?;
        Some(format!(
            "Playback Source replaced before it started: {} load_to_replaced_ms={}",
            replaced.fields(),
            replaced.sent_at.elapsed().as_millis()
        ))
    }

    /// The line to report once the load plays or fails.
    pub(crate) fn observe(&self, mark: LoadMark) -> Option<String> {
        let mut current = self.0.lock().ok()?;
        let load = current.as_mut()?;
        match mark {
            LoadMark::Accepted(revision) => {
                if load.revision == Some(revision) {
                    load.revision = None;
                }
                None
            }
            _ if load.revision.is_some() => None,
            LoadMark::FileLoaded => {
                load.file_loaded.get_or_insert(load.sent_at.elapsed());
                None
            }
            LoadMark::Playing => {
                let load = current.take()?;
                let file_loaded = load
                    .file_loaded
                    .map(|elapsed| format!(" load_to_file_loaded_ms={}", elapsed.as_millis()))
                    .unwrap_or_default();
                Some(format!(
                    "Playback Source started: {}{file_loaded} load_to_playing_ms={}",
                    load.fields(),
                    load.sent_at.elapsed().as_millis()
                ))
            }
            LoadMark::Failed => {
                let load = current.take()?;
                Some(format!(
                    "Playback Source failed to start: {} load_to_failed_ms={}",
                    load.fields(),
                    load.sent_at.elapsed().as_millis()
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(line: &str) -> Vec<&str> {
        line.split_whitespace()
            .filter(|field| field.contains("_ms="))
            .map(|field| field.split('=').next().expect("name"))
            .collect()
    }

    #[test]
    fn reports_a_load_once_when_it_plays() {
        let timer = LoadTimer::default();
        assert_eq!(
            timer.start(Some(2), "external-track", LoadTrigger::Play),
            None
        );
        // Events before mpv accepted this load belong to the previous file.
        assert_eq!(timer.observe(LoadMark::Playing), None);
        assert_eq!(timer.observe(LoadMark::Accepted(1)), None);
        assert_eq!(timer.observe(LoadMark::FileLoaded), None);
        assert_eq!(timer.observe(LoadMark::Accepted(2)), None);
        assert_eq!(timer.observe(LoadMark::FileLoaded), None);
        let line = timer.observe(LoadMark::Playing).expect("line");
        assert!(
            line.starts_with("Playback Source started: type=external-track start=play "),
            "{line}"
        );
        assert_eq!(
            numbers(&line),
            ["load_to_file_loaded_ms", "load_to_playing_ms"]
        );
        // A seek's playback-restart later is not a load.
        assert_eq!(timer.observe(LoadMark::Playing), None);
    }

    #[test]
    fn reports_a_load_that_fails_and_one_replaced() {
        let timer = LoadTimer::default();
        timer.start(Some(1), "track", LoadTrigger::Play);
        timer.observe(LoadMark::Accepted(1));
        let replaced = timer
            .start(Some(2), "radio-station", LoadTrigger::Play)
            .expect("replaced line");
        assert!(
            replaced.starts_with(
                "Playback Source replaced before it started: type=track start=play load_to_replaced_ms="
            ),
            "{replaced}"
        );
        timer.observe(LoadMark::Accepted(2));
        let line = timer.observe(LoadMark::Failed).expect("line");
        assert!(
            line.starts_with(
                "Playback Source failed to start: type=radio-station start=play load_to_failed_ms="
            ),
            "{line}"
        );
        assert_eq!(timer.observe(LoadMark::Playing), None);
    }

    #[test]
    fn a_load_without_a_boundary_counts_the_next_events() {
        let timer = LoadTimer::default();
        assert_eq!(timer.start(None, "track", LoadTrigger::Gapless), None);
        assert_eq!(timer.observe(LoadMark::FileLoaded), None);
        let line = timer.observe(LoadMark::Playing).expect("line");
        assert!(
            line.starts_with("Playback Source started: type=track start=gapless "),
            "{line}"
        );
        // Another load's boundary does not hold it back.
        timer.start(None, "track", LoadTrigger::Queue);
        assert_eq!(timer.observe(LoadMark::Accepted(7)), None);
        let line = timer.observe(LoadMark::Playing).expect("line");
        assert!(line.contains(" start=queue "), "{line}");
    }
}
