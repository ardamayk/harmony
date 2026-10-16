//! MPRIS (org.mpris.MediaPlayer2) integration for Linux desktops: media
//! widgets, `playerctl` and headset buttons drive playback through D-Bus.
//!
//! The D-Bus server runs on the Tauri async runtime. Playback state arrives
//! over a channel from the synchronous mpv event thread; a pure `MprisView`
//! is derived from it, diffed against the previous view, and only the changed
//! properties are announced. Incoming commands are translated into
//! `DesktopPlaybackAction`s and dispatched on a blocking thread, because a
//! transition fade may sleep.

use crate::playback::{PlaybackSessionState, PlaybackStatus as SessionStatus, RepeatMode};
use crate::playback_app_actions::DesktopPlaybackAction;
use mpris_server::zbus::{self, fdo};
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Signal, Time, TrackId, Volume,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const BUS_NAME_SUFFIX: &str = "earthly_audio";
const IDENTITY: &str = "Earthly Audio";
const DESKTOP_ENTRY: &str = "earthly-audio";
const TRACK_ID_PREFIX: &str = "/org/earthly_audio/track/";
/// Position changes larger than this between two state updates are reported
/// as a seek; smaller ones are ordinary playback progress.
const SEEK_THRESHOLD_MICROS: i64 = 2_000_000;

pub(crate) type MprisDispatch = Arc<dyn Fn(DesktopPlaybackAction) + Send + Sync>;

/// What MPRIS clients see, derived from the playback session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MprisView {
    pub(crate) status: PlaybackStatus,
    pub(crate) loop_status: LoopStatus,
    pub(crate) shuffle: bool,
    pub(crate) rate: PlaybackRate,
    pub(crate) volume: Volume,
    pub(crate) position_micros: i64,
    pub(crate) track: Option<MprisTrack>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MprisTrack {
    pub(crate) track_id: String,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) art_url: Option<String>,
    pub(crate) length_micros: Option<i64>,
    pub(crate) can_seek: bool,
}

impl Default for MprisView {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            loop_status: LoopStatus::None,
            shuffle: false,
            rate: 1.0,
            volume: 1.0,
            position_micros: 0,
            track: None,
        }
    }
}

impl MprisView {
    /// `art_base_url` is the private media proxy origin; covers are served
    /// from it so desktop widgets can fetch them without the app's auth.
    pub(crate) fn from_state(state: &PlaybackSessionState, art_base_url: &str) -> Self {
        Self {
            status: match state.status {
                SessionStatus::Playing | SessionStatus::Reconnecting => PlaybackStatus::Playing,
                SessionStatus::Paused => PlaybackStatus::Paused,
                SessionStatus::Idle | SessionStatus::Ended | SessionStatus::Error => {
                    PlaybackStatus::Stopped
                }
            },
            loop_status: match state.repeat_mode {
                RepeatMode::Off => LoopStatus::None,
                RepeatMode::Once => LoopStatus::Track,
                RepeatMode::Loop => LoopStatus::Playlist,
            },
            shuffle: state.shuffle_enabled,
            rate: state.playback_rate,
            volume: state.volume,
            position_micros: seconds_to_micros(state.current_time),
            track: state
                .source
                .as_ref()
                .and_then(|source| MprisTrack::from_source(source, state.duration, art_base_url)),
        }
    }

    pub(crate) fn metadata(&self) -> Metadata {
        let Some(track) = self.track.as_ref() else {
            return Metadata::builder().trackid(TrackId::NO_TRACK).build();
        };
        let mut builder = Metadata::builder().title(track.title.clone());
        if let Ok(track_id) = TrackId::try_from(track.track_id.as_str()) {
            builder = builder.trackid(track_id);
        }
        if let Some(artist) = track.artist.as_ref() {
            builder = builder.artist([artist.clone()]);
        }
        if let Some(album) = track.album.as_ref() {
            builder = builder.album(album.clone());
        }
        if let Some(art_url) = track.art_url.as_ref() {
            builder = builder.art_url(art_url.clone());
        }
        if let Some(length) = track.length_micros {
            builder = builder.length(Time::from_micros(length));
        }
        builder.build()
    }

    fn has_source(&self) -> bool {
        self.track.is_some()
    }

    fn can_seek(&self) -> bool {
        self.track.as_ref().is_some_and(|track| track.can_seek)
    }

    /// Properties whose value differs from `previous`, ready to announce.
    pub(crate) fn changed_properties(&self, previous: &Self) -> Vec<Property> {
        let mut changed = Vec::new();
        if self.status != previous.status {
            changed.push(Property::PlaybackStatus(self.status));
        }
        if self.loop_status != previous.loop_status {
            changed.push(Property::LoopStatus(self.loop_status));
        }
        if self.shuffle != previous.shuffle {
            changed.push(Property::Shuffle(self.shuffle));
        }
        if self.rate != previous.rate {
            changed.push(Property::Rate(self.rate));
        }
        if self.volume != previous.volume {
            changed.push(Property::Volume(self.volume));
        }
        if self.track != previous.track {
            changed.push(Property::Metadata(self.metadata()));
            changed.push(Property::CanGoNext(self.has_source()));
            changed.push(Property::CanGoPrevious(self.has_source()));
            changed.push(Property::CanPlay(self.has_source()));
            changed.push(Property::CanPause(self.has_source()));
            changed.push(Property::CanSeek(self.can_seek()));
        }
        changed
    }

    /// Whether the position moved further than playback alone explains.
    pub(crate) fn seeked_since(&self, previous: &Self) -> bool {
        if self.track != previous.track {
            return false;
        }
        (self.position_micros - previous.position_micros).abs() > SEEK_THRESHOLD_MICROS
    }
}

impl MprisTrack {
    fn from_source(source: &Value, duration_seconds: f64, art_base_url: &str) -> Option<Self> {
        match source.get("type").and_then(Value::as_str)? {
            "track" => {
                let id = source.pointer("/track/id").and_then(Value::as_str)?;
                let title = source.pointer("/track/title").and_then(Value::as_str)?;
                let album_id = source.pointer("/track/albumId").and_then(Value::as_str);
                let length_seconds = if duration_seconds > 0.0 {
                    Some(duration_seconds)
                } else {
                    source
                        .pointer("/track/durationMs")
                        .and_then(Value::as_f64)
                        .filter(|ms| *ms > 0.0)
                        .map(|ms| ms / 1000.0)
                };
                Some(Self {
                    track_id: track_object_path(id),
                    title: title.to_owned(),
                    artist: source
                        .pointer("/track/artistName")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    album: source
                        .pointer("/track/albumTitle")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    art_url: album_id.map(|album_id| {
                        format!("{art_base_url}/api/v1/library/albums/{album_id}/cover")
                    }),
                    length_micros: length_seconds.map(seconds_to_micros),
                    can_seek: true,
                })
            }
            "radio-station" | "catalog-preview" => {
                let pointer = if source.get("type").and_then(Value::as_str) == Some("radio-station")
                {
                    "/station"
                } else {
                    "/result"
                };
                let name = source
                    .pointer(&format!("{pointer}/name"))
                    .and_then(Value::as_str)?;
                let id = source
                    .pointer(&format!("{pointer}/id"))
                    .or_else(|| source.pointer(&format!("{pointer}/stationUuid")))
                    .and_then(Value::as_str)
                    .unwrap_or("radio");
                Some(Self {
                    track_id: track_object_path(&format!("radio-{id}")),
                    title: name.to_owned(),
                    artist: Some("Live radio".to_owned()),
                    album: None,
                    art_url: source
                        .pointer(&format!("{pointer}/faviconUrl"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    length_micros: None,
                    can_seek: false,
                })
            }
            _ => None,
        }
    }
}

/// D-Bus object paths only allow `[A-Za-z0-9_]` segments.
pub(crate) fn track_object_path(id: &str) -> String {
    let sanitized: String = id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{TRACK_ID_PREFIX}{sanitized}")
}

fn seconds_to_micros(seconds: f64) -> i64 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0;
    }
    (seconds * 1_000_000.0).round() as i64
}

/// The D-Bus-facing player: answers property reads from the shared view
/// and forwards commands through the dispatcher.
pub(crate) struct MprisPlayer {
    dispatch: MprisDispatch,
    view: Arc<Mutex<MprisView>>,
}

impl MprisPlayer {
    fn view(&self) -> MprisView {
        self.view
            .lock()
            .map(|view| view.clone())
            .unwrap_or_default()
    }

    fn dispatch(&self, action: DesktopPlaybackAction) {
        let dispatch = self.dispatch.clone();
        crate::runtime::spawn_blocking(move || dispatch(action));
    }
}

impl RootInterface for MprisPlayer {
    async fn raise(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::OpenMainWindow);
        Ok(())
    }

    async fn quit(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Quit);
        Ok(())
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _fullscreen: bool) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok(IDENTITY.to_owned())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(DESKTOP_ENTRY.to_owned())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl PlayerInterface for MprisPlayer {
    async fn next(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Next);
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Previous);
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Pause);
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::TogglePlay);
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Stop);
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        self.dispatch(DesktopPlaybackAction::Play);
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        let view = self.view();
        if !view.can_seek() {
            return Ok(());
        }
        let target = (view.position_micros + offset.as_micros()).max(0);
        self.dispatch(DesktopPlaybackAction::SeekTo(target as f64 / 1_000_000.0));
        Ok(())
    }

    async fn set_position(&self, track_id: TrackId, position: Time) -> fdo::Result<()> {
        let view = self.view();
        let Some(track) = view.track.as_ref() else {
            return Ok(());
        };
        // MPRIS asks players to ignore positions meant for another track.
        if track.track_id != track_id.as_str() || !track.can_seek {
            return Ok(());
        }
        self.dispatch(DesktopPlaybackAction::SeekTo(
            position.as_micros().max(0) as f64 / 1_000_000.0,
        ));
        Ok(())
    }

    async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(
            "Earthly Audio plays from its own library.".to_owned(),
        ))
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(self.view().status)
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(self.view().loop_status)
    }

    async fn set_loop_status(&self, _loop_status: LoopStatus) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(self.view().rate)
    }

    async fn set_rate(&self, rate: PlaybackRate) -> zbus::Result<()> {
        let supported_rates =
            crate::playback::MIN_PLAYBACK_RATE..=crate::playback::MAX_PLAYBACK_RATE;
        if !rate.is_finite() || !supported_rates.contains(&rate) {
            return Err(fdo::Error::InvalidArgs(
                "Playback rate must be finite and within the advertised range.".to_owned(),
            )
            .into());
        }
        self.dispatch(DesktopPlaybackAction::SetRate(rate));
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(self.view().shuffle)
    }

    async fn set_shuffle(&self, _shuffle: bool) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(self.view().metadata())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.view().volume)
    }

    async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
        self.dispatch(DesktopPlaybackAction::SetVolume(volume.clamp(0.0, 1.0)));
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_micros(self.view().position_micros))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(crate::playback::MIN_PLAYBACK_RATE)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(crate::playback::MAX_PLAYBACK_RATE)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(self.view().has_source())
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(self.view().has_source())
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(self.view().has_source())
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(self.view().has_source())
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(self.view().can_seek())
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

/// Handle held by the app: pushes playback state to the D-Bus task.
#[derive(Clone)]
pub(crate) struct MprisBridge {
    sender: mpsc::UnboundedSender<PlaybackSessionState>,
}

impl MprisBridge {
    /// Starts the D-Bus server on the core runtime. When no session
    /// bus is available the bridge logs once and silently drops updates.
    pub(crate) fn start(dispatch: MprisDispatch, art_base_url: String) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        crate::runtime::spawn(run_server(dispatch, art_base_url, receiver));
        Self { sender }
    }

    pub(crate) fn update(&self, state: PlaybackSessionState) {
        // A closed channel means MPRIS is unavailable; nothing to do.
        let _ = self.sender.send(state);
    }
}

async fn run_server(
    dispatch: MprisDispatch,
    art_base_url: String,
    mut receiver: mpsc::UnboundedReceiver<PlaybackSessionState>,
) {
    let shared = Arc::new(Mutex::new(MprisView::default()));
    let player = MprisPlayer {
        dispatch,
        view: shared.clone(),
    };
    let server = match Server::new(BUS_NAME_SUFFIX, player).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("MPRIS is unavailable (no session bus?): {error}");
            return;
        }
    };
    let mut last = MprisView::default();
    while let Some(state) = receiver.recv().await {
        let view = MprisView::from_state(&state, &art_base_url);
        if let Ok(mut current) = shared.lock() {
            *current = view.clone();
        }
        let changed = view.changed_properties(&last);
        if !changed.is_empty()
            && let Err(error) = server.properties_changed(changed).await
        {
            eprintln!("MPRIS property announcement failed: {error}");
        }
        if view.seeked_since(&last)
            && let Err(error) = server
                .emit(Signal::Seeked {
                    position: Time::from_micros(view.position_micros),
                })
                .await
        {
            eprintln!("MPRIS Seeked signal failed: {error}");
        }
        last = view;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn set_rate_dispatches_supported_rates_and_rejects_invalid_values() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let player = MprisPlayer {
            dispatch: Arc::new(move |action| sender.send(action).expect("receive action")),
            view: Arc::new(Mutex::new(MprisView::default())),
        };
        let minimum = player.minimum_rate().await.expect("minimum rate");
        let maximum = player.maximum_rate().await.expect("maximum rate");
        for rate in [minimum, 1.25, maximum] {
            player.set_rate(rate).await.expect("supported rate");
            let action = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
                .await
                .expect("dispatch completed");
            assert_eq!(action, Some(DesktopPlaybackAction::SetRate(rate)));
        }
        for rate in [
            minimum - 0.1,
            maximum + 0.1,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(player.set_rate(rate).await.is_err());
        }
        assert!(receiver.try_recv().is_err());
    }

    fn track_state() -> PlaybackSessionState {
        let mut state = PlaybackSessionState::default();
        state.source = Some(json!({
            "type": "track",
            "track": {
                "id": "9f0b-track",
                "title": "Nemo",
                "artistName": "Nightwish",
                "albumTitle": "Once",
                "albumId": "album-1",
                "durationMs": 276000
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/9f0b-track/stream"
        }));
        state.status = SessionStatus::Playing;
        state.current_time = 12.5;
        state.duration = 276.0;
        state.volume = 0.6;
        state.shuffle_enabled = true;
        state.repeat_mode = RepeatMode::Loop;
        state.playback_rate = 1.25;
        state
    }

    #[test]
    fn view_maps_session_state_to_mpris_properties() {
        let view = MprisView::from_state(&track_state(), "http://127.0.0.1:43129/token");

        assert_eq!(view.status, PlaybackStatus::Playing);
        assert_eq!(view.loop_status, LoopStatus::Playlist);
        assert!(view.shuffle);
        assert_eq!(view.rate, 1.25);
        assert_eq!(view.volume, 0.6);
        assert_eq!(view.position_micros, 12_500_000);
        let track = view.track.as_ref().expect("track");
        assert_eq!(track.track_id, "/org/earthly_audio/track/9f0b_track");
        assert_eq!(track.title, "Nemo");
        assert_eq!(track.artist.as_deref(), Some("Nightwish"));
        assert_eq!(track.album.as_deref(), Some("Once"));
        assert_eq!(
            track.art_url.as_deref(),
            Some("http://127.0.0.1:43129/token/api/v1/library/albums/album-1/cover")
        );
        assert_eq!(track.length_micros, Some(276_000_000));
        assert!(track.can_seek);

        let metadata = view.metadata();
        assert_eq!(metadata.title(), Some("Nemo"));
        assert_eq!(metadata.length(), Some(Time::from_micros(276_000_000)));
        assert_eq!(
            metadata.trackid().map(|id| id.as_str().to_owned()),
            Some("/org/earthly_audio/track/9f0b_track".to_owned())
        );
    }

    #[test]
    fn view_reports_stopped_without_a_source_and_live_radio_as_unseekable() {
        let idle = MprisView::from_state(&PlaybackSessionState::default(), "http://x");
        assert_eq!(idle.status, PlaybackStatus::Stopped);
        assert!(idle.track.is_none());
        assert_eq!(idle.metadata().trackid(), Some(TrackId::NO_TRACK));

        let mut state = PlaybackSessionState::default();
        state.status = SessionStatus::Reconnecting;
        state.source = Some(json!({
            "type": "radio-station",
            "station": { "id": "st-1", "name": "Radio Paradise", "faviconUrl": "https://rp.example/icon.png" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/st-1/stream",
            "sourceUrl": "https://rp.example/live"
        }));
        let radio = MprisView::from_state(&state, "http://x");
        assert_eq!(radio.status, PlaybackStatus::Playing);
        let track = radio.track.expect("radio track");
        assert_eq!(track.title, "Radio Paradise");
        assert_eq!(track.artist.as_deref(), Some("Live radio"));
        assert_eq!(
            track.art_url.as_deref(),
            Some("https://rp.example/icon.png")
        );
        assert!(!track.can_seek);
    }

    #[test]
    fn changed_properties_only_announce_differences_and_detect_seeks() {
        let previous = MprisView::from_state(&track_state(), "http://x");
        let mut state = track_state();
        state.current_time = 13.0;
        let progressed = MprisView::from_state(&state, "http://x");
        assert!(progressed.changed_properties(&previous).is_empty());
        assert!(!progressed.seeked_since(&previous));

        state.current_time = 90.0;
        state.status = SessionStatus::Paused;
        state.volume = 0.3;
        let jumped = MprisView::from_state(&state, "http://x");
        let changed = jumped.changed_properties(&previous);
        assert!(matches!(
            changed[0],
            Property::PlaybackStatus(PlaybackStatus::Paused)
        ));
        assert!(matches!(changed[1], Property::Volume(volume) if volume == 0.3));
        assert_eq!(changed.len(), 2);
        assert!(jumped.seeked_since(&previous));

        let mut next_track = track_state();
        next_track.source = Some(json!({
            "type": "track",
            "track": { "id": "other", "title": "Other", "albumId": "album-2" },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/other/stream"
        }));
        next_track.current_time = 0.0;
        let switched = MprisView::from_state(&next_track, "http://x");
        let changed = switched.changed_properties(&previous);
        assert!(
            changed
                .iter()
                .any(|property| matches!(property, Property::Metadata(_)))
        );
        assert!(
            !switched.seeked_since(&previous),
            "a new track is not a seek"
        );
    }

    #[test]
    fn object_paths_only_contain_allowed_characters() {
        assert_eq!(
            track_object_path("a-b.c d"),
            "/org/earthly_audio/track/a_b_c_d"
        );
        assert!(TrackId::try_from(track_object_path("uuid-1234").as_str()).is_ok());
    }
}
