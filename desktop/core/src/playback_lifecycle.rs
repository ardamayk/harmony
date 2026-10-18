use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const ALLOWED_REPEAT_MODES: &[&str] = &["off", "once", "loop"];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackSessionSnapshot {
    source: Option<Value>,
    playhead_seconds: f64,
    volume: f64,
    shuffle_enabled: bool,
    repeat_mode: String,
}

impl PlaybackSessionSnapshot {
    pub(crate) fn new(
        source: Option<Value>,
        playhead_seconds: f64,
        volume: f64,
        shuffle_enabled: bool,
        repeat_mode: impl Into<String>,
    ) -> Result<Self, PlaybackLifecycleError> {
        let repeat_mode = repeat_mode.into();
        validate_snapshot(playhead_seconds, volume, &repeat_mode)?;
        Ok(Self {
            source,
            playhead_seconds,
            volume,
            shuffle_enabled,
            repeat_mode,
        })
    }

    pub(crate) fn from_serializable_state(
        state: &impl Serialize,
    ) -> Result<Self, PlaybackLifecycleError> {
        let value = serde_json::to_value(state)
            .map_err(|error| PlaybackLifecycleError::InvalidState(error.to_string()))?;
        let state = serde_json::from_value::<PlaybackStateProjection>(value)
            .map_err(|error| PlaybackLifecycleError::InvalidState(error.to_string()))?;
        Self::new(
            state.source,
            state.current_time,
            state.volume,
            state.shuffle_enabled,
            state.repeat_mode,
        )
    }

    pub(crate) fn source(&self) -> Option<&Value> {
        self.source.as_ref()
    }

    pub(crate) fn rebind_media_proxy(
        &self,
        media_proxy_base_url: &str,
    ) -> Result<Self, PlaybackLifecycleError> {
        let Some(mut source) = self.source.clone() else {
            return Ok(self.clone());
        };
        let path = playback_source_path(&source)?;
        let playback_url = format!("{media_proxy_base_url}/{path}");
        source
            .as_object_mut()
            .ok_or_else(|| invalid_source("Playback Source must be an object."))?
            .insert("playbackUrl".to_owned(), Value::String(playback_url));
        let mut snapshot = self.clone();
        snapshot.source = Some(source);
        Ok(snapshot)
    }

    pub(crate) fn playhead_seconds(&self) -> f64 {
        self.playhead_seconds
    }

    pub(crate) fn volume(&self) -> f64 {
        self.volume
    }

    pub(crate) fn is_shuffle_enabled(&self) -> bool {
        self.shuffle_enabled
    }

    pub(crate) fn repeat_mode(&self) -> &str {
        &self.repeat_mode
    }
}

fn playback_source_path(source: &Value) -> Result<String, PlaybackLifecycleError> {
    let (identifier_pointer, path_prefix) = match source.get("type").and_then(Value::as_str) {
        Some("track") => ("/track/id", "api/v1/tracks"),
        Some("radio-station") => ("/station/id", "api/v1/radio/stations"),
        Some("catalog-preview") => ("/result/stationUuid", "api/v1/radio/preview"),
        _ => return Err(invalid_source("Playback Source type is unsupported.")),
    };
    let identifier = source
        .pointer(identifier_pointer)
        .and_then(Value::as_str)
        .filter(|value| is_safe_path_segment(value))
        .ok_or_else(|| invalid_source("Playback Source identifier is invalid."))?;
    Ok(format!("{path_prefix}/{identifier}/stream"))
}

fn is_safe_path_segment(value: &str) -> bool {
    !value.is_empty() && !value.contains(['/', '?', '#'])
}

fn invalid_source(message: &str) -> PlaybackLifecycleError {
    PlaybackLifecycleError::InvalidSnapshot(message.to_owned())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackStateProjection {
    source: Option<Value>,
    current_time: f64,
    volume: f64,
    shuffle_enabled: bool,
    repeat_mode: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlaybackLifecycleState {
    Foreground,
    Background,
    Recovering,
    Failed,
    Quitting,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlaybackLifecycleAction {
    KeepPlayerRunning,
    PublishPlaybackState,
    RestartPlayer {
        snapshot: PlaybackSessionSnapshot,
        should_resume: bool,
    },
    SurfaceActionableError(PlaybackLifecycleFailure),
    StopPlayerAndExit,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackLifecycleTransition {
    pub(crate) state: PlaybackLifecycleState,
    pub(crate) action: PlaybackLifecycleAction,
}

pub(crate) struct PlaybackLifecycle {
    state: PlaybackLifecycleState,
    has_recovery_attempted: bool,
}

impl PlaybackLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            state: PlaybackLifecycleState::Foreground,
            has_recovery_attempted: false,
        }
    }

    pub(crate) fn close_main_window(&mut self) -> PlaybackLifecycleTransition {
        self.state = PlaybackLifecycleState::Background;
        self.transition(PlaybackLifecycleAction::KeepPlayerRunning)
    }

    pub(crate) fn renderer_attached(&self) -> PlaybackLifecycleTransition {
        self.transition(PlaybackLifecycleAction::PublishPlaybackState)
    }

    pub(crate) fn explicit_quit(
        &mut self,
        store: &PlaybackSnapshotStore,
        snapshot: &PlaybackSessionSnapshot,
    ) -> Result<PlaybackLifecycleTransition, PlaybackLifecycleError> {
        store.save(snapshot)?;
        self.state = PlaybackLifecycleState::Quitting;
        Ok(self.transition(PlaybackLifecycleAction::StopPlayerAndExit))
    }

    pub(crate) fn unexpected_player_exit(
        &mut self,
        snapshot: &PlaybackSessionSnapshot,
        should_resume: bool,
    ) -> PlaybackLifecycleTransition {
        if !self.has_recovery_attempted {
            self.has_recovery_attempted = true;
            self.state = PlaybackLifecycleState::Recovering;
            return self.transition(PlaybackLifecycleAction::RestartPlayer {
                snapshot: snapshot.clone(),
                should_resume,
            });
        }
        self.state = PlaybackLifecycleState::Failed;
        self.transition(PlaybackLifecycleAction::SurfaceActionableError(
            PlaybackLifecycleFailure::crash_loop(),
        ))
    }

    pub(crate) fn player_stabilized(&mut self) {
        self.has_recovery_attempted = false;
        if self.state == PlaybackLifecycleState::Recovering {
            self.state = PlaybackLifecycleState::Foreground;
        }
    }

    fn transition(&self, action: PlaybackLifecycleAction) -> PlaybackLifecycleTransition {
        PlaybackLifecycleTransition {
            state: self.state,
            action,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackLifecycleFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl PlaybackLifecycleFailure {
    fn crash_loop() -> Self {
        Self {
            code: "mpv-crash-loop",
            message: "Native playback stopped after two consecutive mpv failures. Quit and reopen the Desktop Client to try again.".to_owned(),
        }
    }
}

fn validate_snapshot(
    playhead_seconds: f64,
    volume: f64,
    repeat_mode: &str,
) -> Result<(), PlaybackLifecycleError> {
    if !playhead_seconds.is_finite() || playhead_seconds < 0.0 {
        return Err(PlaybackLifecycleError::InvalidSnapshot(
            "Playback Session Snapshot playhead must be a non-negative finite number.".to_owned(),
        ));
    }
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(PlaybackLifecycleError::InvalidSnapshot(
            "Playback Session Snapshot volume must be between zero and one.".to_owned(),
        ));
    }
    if !ALLOWED_REPEAT_MODES.contains(&repeat_mode) {
        return Err(PlaybackLifecycleError::InvalidSnapshot(format!(
            "Playback Session Snapshot repeat mode '{repeat_mode}' is unsupported."
        )));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct PlaybackSnapshotStore {
    path: PathBuf,
}

impl PlaybackSnapshotStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load(&self) -> Result<Option<PlaybackSessionSnapshot>, PlaybackLifecycleError> {
        let contents = match fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PlaybackLifecycleError::read(&self.path, error)),
        };
        let snapshot = serde_json::from_slice::<PlaybackSessionSnapshot>(&contents)
            .map_err(|error| PlaybackLifecycleError::decode(&self.path, error))?;
        validate_snapshot(
            snapshot.playhead_seconds,
            snapshot.volume,
            &snapshot.repeat_mode,
        )?;
        Ok(Some(snapshot))
    }

    pub(crate) fn save(
        &self,
        snapshot: &PlaybackSessionSnapshot,
    ) -> Result<(), PlaybackLifecycleError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| PlaybackLifecycleError::write(&self.path, error))?;
        let contents = serde_json::to_vec_pretty(snapshot)
            .map_err(|error| PlaybackLifecycleError::encode(&self.path, error))?;
        let temporary_path = self.path.with_extension("json.tmp");
        fs::write(&temporary_path, contents)
            .map_err(|error| PlaybackLifecycleError::write(&temporary_path, error))?;
        fs::rename(&temporary_path, &self.path)
            .map_err(|error| PlaybackLifecycleError::write(&self.path, error))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlaybackLifecycleError {
    #[error("{0}")]
    InvalidSnapshot(String),
    #[error("Playback Session state could not be snapshotted: {0}")]
    InvalidState(String),
    #[error("Playback Session Snapshot at {path} could not be read: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("Playback Session Snapshot at {path} is invalid: {source}")]
    Decode {
        path: String,
        source: serde_json::Error,
    },
    #[error("Playback Session Snapshot at {path} could not be encoded: {source}")]
    Encode {
        path: String,
        source: serde_json::Error,
    },
    #[error("Playback Session Snapshot at {path} could not be written: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
}

impl PlaybackLifecycleError {
    fn read(path: &Path, source: std::io::Error) -> Self {
        Self::Read {
            path: path.display().to_string(),
            source,
        }
    }

    fn decode(path: &Path, source: serde_json::Error) -> Self {
        Self::Decode {
            path: path.display().to_string(),
            source,
        }
    }

    fn encode(path: &Path, source: serde_json::Error) -> Self {
        Self::Encode {
            path: path.display().to_string(),
            source,
        }
    }

    fn write(path: &Path, source: std::io::Error) -> Self {
        Self::Write {
            path: path.display().to_string(),
            source,
        }
    }
}
