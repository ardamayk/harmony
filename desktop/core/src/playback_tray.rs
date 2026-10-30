use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Tooltip updates carry the playhead, which changes several times a second;
/// D-Bus tray hosts are only told this often.
const TOOLTIP_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// What the shell's tray shows. The shell draws the menu; the core decides
/// its labels and only sends an update when something visible changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrayUpdate {
    pub(crate) source_label: String,
    pub(crate) has_source: bool,
    pub(crate) previous_label: &'static str,
    pub(crate) toggle_label: &'static str,
    pub(crate) next_label: &'static str,
    pub(crate) open_label: &'static str,
    pub(crate) mini_label: &'static str,
    pub(crate) quit_label: &'static str,
    /// Present only when the tooltip is due (see [`TooltipThrottle`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tooltip: Option<String>,
}

/// Coalesces Playback Session updates into tray updates.
#[derive(Default)]
pub(crate) struct PlaybackTray {
    last_menu: Mutex<Option<TrayUpdate>>,
    tooltip: Mutex<TooltipThrottle>,
}

impl PlaybackTray {
    pub(crate) fn update(
        &self,
        source: Option<&Value>,
        is_playing: bool,
        position: PlaybackPosition,
    ) -> Option<TrayUpdate> {
        let view = PlaybackTrayView::from_playback(source, is_playing);
        let tooltip = view.tooltip(position);
        let menu = TrayUpdate {
            source_label: view.source_label,
            has_source: view.has_source,
            previous_label: view.previous_label,
            toggle_label: view.toggle_label,
            next_label: view.next_label,
            open_label: view.open_label,
            mini_label: view.mini_label,
            quit_label: view.quit_label,
            tooltip: None,
        };
        let menu_changed = self
            .last_menu
            .lock()
            .map(|mut last| {
                let changed = last.as_ref() != Some(&menu);
                *last = Some(menu.clone());
                changed
            })
            .unwrap_or(true);
        let tooltip_due = self
            .tooltip
            .lock()
            .map(|mut throttle| throttle.should_send(&tooltip, Instant::now()))
            .unwrap_or(true);
        if !menu_changed && !tooltip_due {
            return None;
        }
        Some(TrayUpdate {
            tooltip: tooltip_due.then_some(tooltip),
            ..menu
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct TooltipThrottle {
    last_text: Option<String>,
    last_sent_at: Option<Instant>,
}

impl TooltipThrottle {
    /// Whether `text` should reach the tray now: always on a change of the
    /// source or playing state (the part before the separator), otherwise at
    /// most once per interval.
    pub(crate) fn should_send(&mut self, text: &str, now: Instant) -> bool {
        let heading_changed = self
            .last_text
            .as_deref()
            .map(tooltip_heading)
            .is_none_or(|previous| previous != tooltip_heading(text));
        let is_due = self
            .last_sent_at
            .is_none_or(|sent| now.duration_since(sent) >= TOOLTIP_REFRESH_INTERVAL);
        if !heading_changed && (!is_due || self.last_text.as_deref() == Some(text)) {
            return false;
        }
        self.last_text = Some(text.to_owned());
        self.last_sent_at = Some(now);
        true
    }
}

fn tooltip_heading(text: &str) -> &str {
    text.split(" · ").next().unwrap_or(text)
}

/// Playhead and length in seconds, for the tooltip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PlaybackPosition {
    pub(crate) current_seconds: f64,
    pub(crate) duration_seconds: f64,
}

pub(crate) fn format_clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_owned();
    }
    let total = seconds.floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackTrayView {
    pub(crate) source_label: String,
    pub(crate) artist_label: Option<String>,
    pub(crate) is_playing: bool,
    pub(crate) has_source: bool,
    pub(crate) previous_label: &'static str,
    pub(crate) toggle_label: &'static str,
    pub(crate) next_label: &'static str,
    pub(crate) open_label: &'static str,
    pub(crate) mini_label: &'static str,
    pub(crate) quit_label: &'static str,
}

impl PlaybackTrayView {
    pub(crate) fn from_playback(source: Option<&Value>, is_playing: bool) -> Self {
        Self {
            source_label: source
                .and_then(source_label)
                .unwrap_or("Nothing playing")
                .to_owned(),
            artist_label: source.and_then(artist_label).map(str::to_owned),
            is_playing,
            has_source: source.is_some(),
            previous_label: "Previous",
            toggle_label: if is_playing { "Pause" } else { "Play" },
            next_label: "Next",
            open_label: "Open Harmony",
            mini_label: "Mini player",
            quit_label: "Quit Harmony",
        }
    }

    /// "Title – Artist · 1:23 / 4:56", "Paused: …", or the app name.
    pub(crate) fn tooltip(&self, position: PlaybackPosition) -> String {
        if !self.has_source {
            return "Harmony".to_owned();
        }
        let mut heading = self.source_label.clone();
        if let Some(artist) = self.artist_label.as_deref() {
            heading.push_str(" – ");
            heading.push_str(artist);
        }
        if !self.is_playing {
            heading = format!("Paused: {heading}");
        }
        if position.duration_seconds > 0.0 {
            format!(
                "{heading} · {} / {}",
                format_clock(position.current_seconds),
                format_clock(position.duration_seconds)
            )
        } else {
            heading
        }
    }

    /// Main window title: the track while something plays, else the app name.
    pub(crate) fn window_title(&self) -> String {
        if !self.has_source {
            return "Harmony".to_owned();
        }
        match self.artist_label.as_deref() {
            Some(artist) => format!("{} – {artist} · Harmony", self.source_label),
            None => format!("{} · Harmony", self.source_label),
        }
    }
}

fn artist_label(source: &Value) -> Option<&str> {
    match source.get("type").and_then(Value::as_str) {
        Some("track") => source.pointer("/track/artistName").and_then(Value::as_str),
        Some("external-track") => source
            .pointer("/externalTrack/artistName")
            .and_then(Value::as_str),
        _ => None,
    }
}

fn source_label(source: &Value) -> Option<&str> {
    match source.get("type").and_then(Value::as_str) {
        Some("track") => source.pointer("/track/title").and_then(Value::as_str),
        Some("radio-station") => source.pointer("/station/name").and_then(Value::as_str),
        Some("catalog-preview") => source.pointer("/result/name").and_then(Value::as_str),
        Some("external-track") => source
            .pointer("/externalTrack/title")
            .and_then(Value::as_str),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn track() -> Value {
        json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Nemo", "artistName": "Nightwish" }
        })
    }

    #[test]
    fn tooltip_names_the_track_state_and_position() {
        let playing = PlaybackTrayView::from_playback(Some(&track()), true);
        assert_eq!(
            playing.tooltip(PlaybackPosition {
                current_seconds: 754.0,
                duration_seconds: 2700.0
            }),
            "Nemo – Nightwish · 12:34 / 45:00"
        );
        let paused = PlaybackTrayView::from_playback(Some(&track()), false);
        assert_eq!(
            paused.tooltip(PlaybackPosition::default()),
            "Paused: Nemo – Nightwish"
        );
        assert_eq!(
            PlaybackTrayView::from_playback(None, false).tooltip(PlaybackPosition::default()),
            "Harmony"
        );
    }

    #[test]
    fn window_title_follows_the_source() {
        assert_eq!(
            PlaybackTrayView::from_playback(Some(&track()), true).window_title(),
            "Nemo – Nightwish · Harmony"
        );
        assert_eq!(
            PlaybackTrayView::from_playback(None, false).window_title(),
            "Harmony"
        );
    }

    #[test]
    fn external_tracks_name_their_title_and_artist() {
        let external = json!({
            "type": "external-track",
            "externalTrack": { "id": "token.signature", "title": "Around the World", "artistName": "Daft Punk" }
        });
        assert_eq!(
            PlaybackTrayView::from_playback(Some(&external), true).window_title(),
            "Around the World – Daft Punk · Harmony"
        );
    }

    #[test]
    fn tooltip_throttle_sends_changes_immediately_but_progress_only_periodically() {
        let mut throttle = TooltipThrottle::default();
        let start = Instant::now();
        assert!(throttle.should_send("Nemo – Nightwish · 0:01 / 4:36", start));
        assert!(!throttle.should_send(
            "Nemo – Nightwish · 0:02 / 4:36",
            start + Duration::from_secs(1)
        ));
        assert!(throttle.should_send(
            "Paused: Nemo – Nightwish · 0:02 / 4:36",
            start + Duration::from_secs(1)
        ));
        assert!(throttle.should_send(
            "Paused: Nemo – Nightwish · 0:09 / 4:36",
            start + Duration::from_secs(7)
        ));
        assert!(!throttle.should_send(
            "Paused: Nemo – Nightwish · 0:09 / 4:36",
            start + Duration::from_secs(20)
        ));
    }
}
