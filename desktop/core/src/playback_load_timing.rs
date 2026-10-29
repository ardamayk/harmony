//! How long a Playback Source takes to start: from sending mpv `loadfile`
//! to `file-loaded`, and to the first `playback-restart`, when audio
//! actually plays. One stderr line per load, naming only the source type,
//! never its URL. A load replaced by the next one before it plays is not
//! reported.

use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    revision: u64,
    source_type: &'static str,
    sent_at: Instant,
    is_accepted: bool,
    file_loaded: Option<Duration>,
}

#[derive(Default)]
pub(crate) struct LoadTimer(Mutex<Option<Load>>);

impl LoadTimer {
    /// Starts timing a load just before its `loadfile` is sent.
    pub(crate) fn start(&self, revision: u64, source_type: &'static str) {
        if let Ok(mut load) = self.0.lock() {
            *load = Some(Load {
                revision,
                source_type,
                sent_at: Instant::now(),
                is_accepted: false,
                file_loaded: None,
            });
        }
    }

    /// The line to report once the load plays or fails.
    pub(crate) fn observe(&self, mark: LoadMark) -> Option<String> {
        let mut current = self.0.lock().ok()?;
        let load = current.as_mut()?;
        match mark {
            LoadMark::Accepted(revision) => {
                load.is_accepted |= revision == load.revision;
                None
            }
            _ if !load.is_accepted => None,
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
                    "Playback Source started: type={}{file_loaded} load_to_playing_ms={}",
                    load.source_type,
                    load.sent_at.elapsed().as_millis()
                ))
            }
            LoadMark::Failed => {
                let load = current.take()?;
                Some(format!(
                    "Playback Source failed to start: type={} load_to_failed_ms={}",
                    load.source_type,
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
        timer.start(2, "external-track");
        // Events before mpv accepted this load belong to the previous file.
        assert_eq!(timer.observe(LoadMark::Playing), None);
        assert_eq!(timer.observe(LoadMark::Accepted(1)), None);
        assert_eq!(timer.observe(LoadMark::FileLoaded), None);
        assert_eq!(timer.observe(LoadMark::Accepted(2)), None);
        assert_eq!(timer.observe(LoadMark::FileLoaded), None);
        let line = timer.observe(LoadMark::Playing).expect("line");
        assert!(
            line.starts_with("Playback Source started: type=external-track "),
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
    fn reports_a_load_that_fails_and_skips_one_replaced() {
        let timer = LoadTimer::default();
        timer.start(1, "track");
        timer.observe(LoadMark::Accepted(1));
        timer.start(2, "radio-station");
        timer.observe(LoadMark::Accepted(2));
        let line = timer.observe(LoadMark::Failed).expect("line");
        assert!(
            line.starts_with(
                "Playback Source failed to start: type=radio-station load_to_failed_ms="
            ),
            "{line}"
        );
        assert_eq!(timer.observe(LoadMark::Playing), None);
    }
}
