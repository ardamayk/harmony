use crate::adaptive_system_rate::AdaptiveSystemRateController;
use crate::exclusive_output::{ExclusiveOutputCoordinator, PipeWireExclusiveOutputCoordinator};
use crate::output_device::{
    AlsaOutputDeviceAdapter, CommandAlsaOutputDeviceAdapter, OutputDevice, OutputDeviceCatalog,
    OutputDeviceError,
};
use crate::playback_lifecycle::{
    PlaybackLifecycle, PlaybackLifecycleAction, PlaybackSessionSnapshot,
};
use crate::playback_navigation::{PlaybackQueueContext, PlaybackSource, QueueDirection};
use crate::processing::{
    AppliedMpvProcessingConfiguration, EffectiveReplayGainMode, MpvProcessingConfiguration,
    OutputMode, ProcessingState, ReplayGainMode,
};
use crate::telemetry::{
    AudioFormatObservation, ObservedMpvProperties, PipeWireObservation, PipeWireObserver,
    PlaybackTelemetry, ProcessingObservation, SourceObservation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_VOLUME: f64 = 0.8;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MPV_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MPV_PINNED_VERSION: &str = include_str!("../mpv-version.txt");
const MPV_START_TIMEOUT: Duration = Duration::from_secs(3);
const MPV_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const PLAYBACK_FAILED_MESSAGE: &str = "Playback failed. Check the source and try again.";
const ADAPTIVE_CLEANUP_FAILED_MESSAGE: &str = "Playback stopped, but Adaptive System Rate could not reset the system audio rate. Restart the Desktop Client.";
const LIVE_RECONNECT_DELAY_STEPS: [u64; 5] = [1, 2, 4, 8, 15];
#[cfg(not(test))]
const LIVE_RECONNECT_READINESS_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const LIVE_RECONNECT_READINESS_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const LIVE_RECONNECT_STABILITY_RESET: Duration = Duration::from_secs(30);
#[cfg(test)]
const LIVE_RECONNECT_STABILITY_RESET: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const DIRECT_OUTPUT_READY_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const DIRECT_OUTPUT_READY_TIMEOUT: Duration = Duration::from_millis(40);
const DIRECT_OUTPUT_RETRY_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(not(test))]
const OUTPUT_DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(test)]
const OUTPUT_DEVICE_REFRESH_INTERVAL: Duration = Duration::from_millis(20);
#[cfg(not(test))]
const PATH_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(test)]
const PATH_REFRESH_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PlaybackStatus {
    Idle,
    Playing,
    Reconnecting,
    Paused,
    Ended,
    Error,
}

impl PlaybackStatus {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Playing | Self::Reconnecting)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RepeatMode {
    Off,
    Once,
    Loop,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackStateError {
    code: String,
    message: String,
}

fn default_playback_rate() -> f64 {
    1.0
}

fn default_is_gapless() -> bool {
    true
}

pub(crate) const MIN_PLAYBACK_RATE: f64 = 0.5;
pub(crate) const MAX_PLAYBACK_RATE: f64 = 2.0;
pub(crate) const MAX_TRANSITION_FADE_MS: u64 = 2000;
const TRANSITION_FADE_STEP: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackSessionState {
    pub(crate) source: Option<Value>,
    pub(crate) output_mode: OutputMode,
    pub(crate) available_output_devices: Vec<OutputDevice>,
    pub(crate) selected_output_device: Option<OutputDevice>,
    pub(crate) output_device_issue: Option<OutputDeviceIssue>,
    pub(crate) status: PlaybackStatus,
    pub(crate) current_time: f64,
    pub(crate) duration: f64,
    /// Seconds of audio the demuxer has ahead of the playhead, when known.
    #[serde(default)]
    pub(crate) buffered_end: Option<f64>,
    pub(crate) volume: f64,
    /// mpv `speed`; pitch is preserved by mpv's default pitch correction.
    #[serde(default = "default_playback_rate")]
    pub(crate) playback_rate: f64,
    /// Sleep timer "after this track": the end-of-file handler pauses instead
    /// of advancing and clears the flag.
    #[serde(default)]
    pub(crate) stop_after_current: bool,
    /// mpv is launched with --gapless-audio, so native playback is gapless.
    #[serde(default = "default_is_gapless")]
    pub(crate) is_gapless: bool,
    pub(crate) shuffle_enabled: bool,
    pub(crate) repeat_mode: RepeatMode,
    error: Option<PlaybackStateError>,
    pub(crate) processing: ProcessingState,
    telemetry: PlaybackTelemetry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputDeviceIssue {
    pub(crate) code: OutputDeviceIssueCode,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OutputDeviceIssueCode {
    BusyOrUnsupported,
    Disconnected,
}

impl OutputDeviceIssueCode {
    #[cfg(test)]
    fn as_str(self) -> &'static str {
        match self {
            Self::BusyOrUnsupported => "busy-or-unsupported",
            Self::Disconnected => "disconnected",
        }
    }
}

impl Default for PlaybackSessionState {
    fn default() -> Self {
        Self {
            source: None,
            output_mode: OutputMode::System,
            available_output_devices: Vec::new(),
            selected_output_device: None,
            output_device_issue: None,
            status: PlaybackStatus::Idle,
            current_time: 0.0,
            duration: 0.0,
            buffered_end: None,
            volume: DEFAULT_VOLUME,
            playback_rate: 1.0,
            stop_after_current: false,
            is_gapless: true,
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            error: None,
            processing: ProcessingState::default(),
            telemetry: PlaybackTelemetry::native_system_output(
                SourceObservation::unknown(),
                ObservedMpvProperties::unknown(),
                None,
                ProcessingObservation::unknown(),
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackCommandError {
    code: &'static str,
    pub(crate) message: String,
}

impl PlaybackCommandError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            code: "playback-failed",
            message: message.into(),
        }
    }
}

pub(crate) enum MpvEvent {
    LoadBoundary(u64),
    Time(f64),
    Duration(f64),
    /// `demuxer-cache-time`: absolute position the demuxer has read up to.
    BufferedEnd(f64),
    Paused(bool),
    Decoder(ObservedMpvProperties),
    Ended,
    Error(String),
    ExitedUnexpectedly(String),
}

pub(crate) trait MpvProcessAdapter: Send + Sync {
    fn load(&self, url: &str) -> Result<(), String>;
    fn load_at(&self, url: &str, seconds: f64) -> Result<(), String> {
        self.load(url)?;
        self.seek(seconds)
    }
    fn set_paused(&self, is_paused: bool) -> Result<(), String>;
    fn seek(&self, seconds: f64) -> Result<(), String>;
    fn set_volume(&self, value: f64) -> Result<(), String>;
    fn set_speed(&self, _rate: f64) -> Result<(), String> {
        Ok(())
    }
    fn load_with_event_boundary(&self, url: &str, load_revision: u64) -> Result<(), String> {
        let _ = load_revision;
        self.load(url)
    }
    fn configure_output(
        &self,
        _configuration: &MpvOutputConfiguration,
    ) -> Result<Option<AudioFormatObservation>, String> {
        Ok(None)
    }
    fn observe_output_format(&self) -> Result<Option<AudioFormatObservation>, String> {
        Ok(None)
    }
    fn apply_processing(
        &self,
        configuration: &MpvProcessingConfiguration,
    ) -> Result<AppliedMpvProcessingConfiguration, String> {
        Ok(AppliedMpvProcessingConfiguration {
            volume_percent: configuration.volume_percent,
            replay_gain_mode: configuration.replay_gain_mode,
            audio_filters: configuration.audio_filters.clone(),
        })
    }
    fn stop(&self) -> Result<(), String>;
    fn shutdown(&self);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MpvOutputConfiguration {
    System,
    DirectAlsa { device_id: String },
}

enum MpvWorkerRequest {
    Command {
        command: Value,
        event_boundary: Option<u64>,
        response: Sender<Result<Value, String>>,
    },
    Shutdown,
}

type StartedMpvProcess = (Box<dyn MpvProcessAdapter>, Receiver<MpvEvent>, PathBuf);
type MpvWorker = (Sender<MpvWorkerRequest>, JoinHandle<()>, Receiver<MpvEvent>);

pub(crate) struct RealMpvProcess {
    requests: Sender<MpvWorkerRequest>,
    worker: Mutex<Option<JoinHandle<()>>>,
    ipc_directory: PathBuf,
    exclusive_output: Arc<dyn ExclusiveOutputCoordinator>,
}

impl RealMpvProcess {
    pub(crate) fn start_default() -> Result<(Box<dyn MpvProcessAdapter>, Receiver<MpvEvent>), String>
    {
        let binary = resolve_mpv_binary();
        let (process, events, _) = Self::start(binary, Vec::new())?;
        Ok((process, events))
    }

    fn start(binary: PathBuf, extra_arguments: Vec<String>) -> Result<StartedMpvProcess, String> {
        ensure_pinned_mpv(&binary)?;
        let ipc_directory = create_private_ipc_directory()?;
        match start_mpv_worker(&binary, &ipc_directory, extra_arguments) {
            Ok((requests, worker, events)) => {
                let process = Self {
                    requests,
                    worker: Mutex::new(Some(worker)),
                    ipc_directory: ipc_directory.clone(),
                    exclusive_output: Arc::new(PipeWireExclusiveOutputCoordinator::new()),
                };
                process.observe_properties()?;
                Ok((Box::new(process), events, ipc_directory))
            }
            Err(error) => {
                remove_ipc_directory(&ipc_directory, "after mpv startup failed");
                Err(error)
            }
        }
    }

    fn observe_properties(&self) -> Result<(), String> {
        self.command(json!(["observe_property", 1, "time-pos"]))?;
        self.command(json!(["observe_property", 2, "duration"]))?;
        self.command(json!(["observe_property", 3, "pause"]))?;
        self.command(json!(["observe_property", 4, "audio-params"]))?;
        self.command(json!(["observe_property", 5, "demuxer-cache-time"]))
            .map(|_| ())
    }

    fn command(&self, command: Value) -> Result<Value, String> {
        self.command_with_event_boundary(command, None)
    }

    fn command_with_event_boundary(
        &self,
        command: Value,
        event_boundary: Option<u64>,
    ) -> Result<Value, String> {
        let (response_sender, response_receiver) = mpsc::channel();
        self.requests
            .send(MpvWorkerRequest::Command {
                command,
                event_boundary,
                response: response_sender,
            })
            .map_err(|_| "Native mpv process is not running.".to_owned())?;
        response_receiver
            .recv_timeout(MPV_COMMAND_TIMEOUT)
            .map_err(|_| "Native mpv command timed out.".to_owned())?
    }

    fn stop_worker(&self) {
        let worker = match self.worker.lock() {
            Ok(mut worker) => worker.take(),
            Err(_) => {
                eprintln!("Pinned mpv worker shutdown failed: worker state is unavailable.");
                None
            }
        };
        let Some(worker) = worker else {
            remove_ipc_directory(&self.ipc_directory, "after repeated mpv shutdown");
            return;
        };
        if self.requests.send(MpvWorkerRequest::Shutdown).is_err() {
            eprintln!("Pinned mpv worker shutdown request failed: worker channel is closed.");
        }
        if worker.join().is_err() {
            eprintln!("Pinned mpv worker shutdown failed: worker thread panicked.");
        }
        remove_ipc_directory(&self.ipc_directory, "during mpv shutdown");
    }

    fn restore_system_output(&self) -> Result<(), String> {
        let disable_result = self.command(json!(["set_property", "aid", "no"]));
        let device_result = self.command(json!(["set_property", "audio-device", "auto"]));
        let release_result = self.exclusive_output.release();
        let enable_result = self.command(json!(["set_property", "aid", "auto"]));
        disable_result?;
        device_result?;
        release_result?;
        enable_result?;
        Ok(())
    }
}

fn remove_ipc_directory(path: &Path, context: &str) {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != ErrorKind::NotFound
    {
        eprintln!(
            "Private mpv IPC cleanup failed {context} at '{}': {error}",
            path.display()
        );
    }
}

/// Pinned mpv sidecar status shown on the Settings page. Reads the binary's
/// version once without starting a playback session.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MpvStatus {
    /// The executable starts and answers `--version`.
    pub available: bool,
    /// The found version equals the pinned version this Desktop Client requires.
    pub pinned: bool,
    pub pinned_version: String,
    pub version: Option<String>,
    pub detail: Option<String>,
}

pub fn probe_mpv_status() -> MpvStatus {
    probe_mpv_binary(&resolve_mpv_binary())
}

fn probe_mpv_binary(binary: &Path) -> MpvStatus {
    let pinned_version = MPV_PINNED_VERSION.trim().to_owned();
    let output = match Command::new(binary).arg("--version").output() {
        Ok(output) => output,
        Err(error) => {
            return MpvStatus {
                available: false,
                pinned: false,
                pinned_version,
                version: None,
                detail: Some(format!("{} could not start: {error}", binary.display())),
            };
        }
    };
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("mpv v"))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest).to_owned());
    let available = output.status.success() && version.is_some();
    let pinned = available && version.as_deref() == Some(pinned_version.as_str());
    MpvStatus {
        available,
        pinned,
        detail: (!pinned).then(|| format!("expected pinned mpv {pinned_version}")),
        pinned_version,
        version,
    }
}

fn resolve_mpv_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("EARTHLY_AUDIO_MPV_PATH") {
        return PathBuf::from(binary);
    }
    match std::env::current_exe() {
        Ok(executable) => {
            if let Some(directory) = executable.parent() {
                let packaged_binary = directory.join("mpv");
                if packaged_binary.is_file() {
                    return packaged_binary;
                }
            } else {
                eprintln!("Packaged mpv lookup failed: current executable has no parent path.");
            }
        }
        Err(error) => {
            eprintln!("Packaged mpv lookup failed: current executable is unknown: {error}")
        }
    }
    let development_binary = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/mpv");
    if development_binary.is_file() {
        return development_binary;
    }
    PathBuf::from("/usr/bin/mpv")
}

impl MpvProcessAdapter for RealMpvProcess {
    fn load(&self, url: &str) -> Result<(), String> {
        self.command(json!(["loadfile", url, "replace"]))
            .map(|_| ())
    }

    fn load_at(&self, url: &str, seconds: f64) -> Result<(), String> {
        self.command(json!([
            "loadfile",
            url,
            "replace",
            -1,
            { "start": seconds.to_string() }
        ]))
        .map(|_| ())
    }

    fn set_paused(&self, is_paused: bool) -> Result<(), String> {
        self.command(json!(["set_property", "pause", is_paused]))
            .map(|_| ())
    }

    fn seek(&self, seconds: f64) -> Result<(), String> {
        self.command(json!(["seek", seconds, "absolute+exact"]))
            .map(|_| ())
    }

    fn set_volume(&self, value: f64) -> Result<(), String> {
        self.command(json!(["set_property", "volume", value * 100.0]))
            .map(|_| ())
    }

    fn set_speed(&self, rate: f64) -> Result<(), String> {
        self.command(json!(["set_property", "speed", rate]))
            .map(|_| ())
    }

    fn load_with_event_boundary(&self, url: &str, load_revision: u64) -> Result<(), String> {
        self.command_with_event_boundary(json!(["loadfile", url, "replace"]), Some(load_revision))
            .map(|_| ())
    }

    fn configure_output(
        &self,
        configuration: &MpvOutputConfiguration,
    ) -> Result<Option<AudioFormatObservation>, String> {
        match configuration {
            MpvOutputConfiguration::System => {
                self.restore_system_output()?;
                Ok(None)
            }
            MpvOutputConfiguration::DirectAlsa { device_id } => {
                self.command(json!(["set_property", "aid", "no"]))?;
                if let Err(error) = self.exclusive_output.acquire(device_id) {
                    return match self.restore_system_output() {
                        Ok(()) => Err(error),
                        Err(restore_error) => Err(format!(
                            "{error} System Output cleanup also failed: {restore_error}"
                        )),
                    };
                }
                let result = self
                    .command(json!([
                        "set_property",
                        "audio-device",
                        format!("alsa/{device_id}")
                    ]))
                    .and_then(|_| self.command(json!(["set_property", "aid", "auto"])));
                if let Err(error) = result {
                    if let Err(restore_error) = self.restore_system_output() {
                        return Err(format!(
                            "{error} System Output cleanup also failed: {restore_error}"
                        ));
                    }
                    return Err(error);
                }
                Ok(None)
            }
        }
    }

    fn observe_output_format(&self) -> Result<Option<AudioFormatObservation>, String> {
        let output = self.command(json!(["get_property", "audio-out-params"]))?;
        Ok(output
            .is_object()
            .then(|| ObservedMpvProperties::from_audio_params(&output).format))
    }

    fn apply_processing(
        &self,
        configuration: &MpvProcessingConfiguration,
    ) -> Result<AppliedMpvProcessingConfiguration, String> {
        let replay_gain = match configuration.replay_gain_mode {
            ReplayGainMode::Off => "no",
            ReplayGainMode::Track => "track",
            ReplayGainMode::Album => "album",
        };
        self.command(json!([
            "set_property",
            "volume",
            configuration.volume_percent
        ]))?;
        self.command(json!(["set_property", "replaygain", replay_gain]))?;
        self.command(json!([
            "set_property",
            "af",
            configuration.audio_filters.join(",")
        ]))?;
        let volume_percent = self
            .command(json!(["get_property", "volume"]))?
            .as_f64()
            .ok_or_else(|| "Pinned mpv returned an invalid effective volume.".to_owned())?;
        let replay_gain_mode = match self
            .command(json!(["get_property_string", "replaygain"]))?
            .as_str()
        {
            Some("no") => ReplayGainMode::Off,
            Some("track") => ReplayGainMode::Track,
            Some("album") => ReplayGainMode::Album,
            _ => return Err("Pinned mpv returned an invalid effective ReplayGain mode.".to_owned()),
        };
        let audio_filters = self
            .command(json!(["get_property_string", "af"]))?
            .as_str()
            .ok_or_else(|| "Pinned mpv returned invalid effective audio filters.".to_owned())?
            .to_owned();
        let audio_filters = if audio_filters.is_empty() {
            Vec::new()
        } else {
            audio_filters.split(',').map(str::to_owned).collect()
        };
        Ok(AppliedMpvProcessingConfiguration {
            volume_percent,
            replay_gain_mode,
            audio_filters,
        })
    }

    fn stop(&self) -> Result<(), String> {
        self.command(json!(["stop"])).map(|_| ())
    }

    fn shutdown(&self) {
        self.stop_worker();
        if let Err(error) = self.exclusive_output.release() {
            eprintln!("Exclusive Output ownership cleanup failed during mpv shutdown: {error}");
        }
    }
}

impl Drop for RealMpvProcess {
    fn drop(&mut self) {
        self.stop_worker();
        if let Err(error) = self.exclusive_output.release() {
            eprintln!("Exclusive Output ownership cleanup failed during mpv teardown: {error}");
        }
    }
}

fn ensure_pinned_mpv(binary: &Path) -> Result<(), String> {
    let status = probe_mpv_binary(binary);
    if !status.available {
        return Err(format!(
            "Pinned mpv could not start: {}",
            status.detail.unwrap_or_default()
        ));
    }
    if !status.pinned {
        return Err(format!(
            "Desktop Client requires pinned mpv {}; set EARTHLY_AUDIO_MPV_PATH to that executable.",
            status.pinned_version
        ));
    }
    Ok(())
}

fn create_private_ipc_directory() -> Result<PathBuf, String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| "Private mpv IPC path could not be created.".to_owned())?;
    let name = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = std::env::temp_dir().join(format!("earthly-audio-mpv-{name}"));
    fs::create_dir(&path)
        .map_err(|error| format!("Private mpv IPC directory could not be created: {error}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Private mpv IPC permissions could not be set: {error}"))?;
    Ok(path)
}

fn start_mpv_worker(
    binary: &Path,
    ipc_directory: &Path,
    extra_arguments: Vec<String>,
) -> Result<MpvWorker, String> {
    let socket_path = ipc_directory.join("control.sock");
    let (stream, child_stream) = UnixStream::pair()
        .map_err(|error| format!("Pinned mpv lifetime connection could not be created: {error}"))?;
    let mut command = Command::new(binary);
    configure_mpv_command(&mut command, &socket_path, extra_arguments);
    command.stdin(Stdio::from(std::os::fd::OwnedFd::from(child_stream)));
    let mut child = command
        .spawn()
        .map_err(|error| format!("Pinned mpv child could not start: {error}"))?;
    drop(command);
    wait_for_mpv_socket(&mut child, &socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Private mpv IPC permissions could not be set: {error}"))?;
    stream
        .set_read_timeout(Some(EVENT_POLL_INTERVAL))
        .map_err(|error| format!("Pinned mpv IPC timeout could not be set: {error}"))?;
    Ok(spawn_mpv_worker(child, stream))
}

fn configure_mpv_command(command: &mut Command, socket_path: &Path, extra_arguments: Vec<String>) {
    command
        .arg("--no-config")
        .arg("--idle=yes")
        .arg("--no-video")
        .arg("--audio-display=no")
        .arg("--audio-client-name=Earthly Audio")
        .arg("--force-window=no")
        .arg("--terminal=no")
        .arg("--input-terminal=no")
        .arg("--gapless-audio=weak")
        // Speed changes must not shift pitch; make mpv's default explicit.
        .arg("--audio-pitch-correction=yes")
        .arg(format!("--volume={}", DEFAULT_VOLUME * 100.0))
        .arg(format!("--input-ipc-server={}", socket_path.display()))
        // mpv exits on EOF even if the Desktop Client is killed without running Drop.
        .arg("--input-ipc-client=fd://0")
        .args(extra_arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
}

fn wait_for_mpv_socket(child: &mut Child, socket_path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + MPV_START_TIMEOUT;
    while Instant::now() < deadline {
        if socket_path.exists() {
            return Ok(());
        }
        if child
            .try_wait()
            .map_err(|error| format!("Pinned mpv status could not be read: {error}"))?
            .is_some()
        {
            return Err("Pinned mpv exited before opening private IPC.".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    }
    terminate_mpv(child);
    Err("Pinned mpv did not open private IPC before startup timeout.".to_owned())
}

fn spawn_mpv_worker(
    child: Child,
    stream: UnixStream,
) -> (Sender<MpvWorkerRequest>, JoinHandle<()>, Receiver<MpvEvent>) {
    let (request_sender, request_receiver) = mpsc::channel();
    let (event_sender, event_receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        run_mpv_worker(child, stream, request_receiver, event_sender);
    });
    (request_sender, worker, event_receiver)
}

fn run_mpv_worker(
    mut child: Child,
    mut stream: UnixStream,
    requests: Receiver<MpvWorkerRequest>,
    events: Sender<MpvEvent>,
) {
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            send_mpv_event(
                &events,
                MpvEvent::ExitedUnexpectedly(format!("Pinned mpv IPC failed: {error}")),
                "reporting IPC clone failure",
            );
            terminate_mpv(&mut child);
            return;
        }
    };
    let mut reader = BufReader::new(reader_stream);
    let mut request_id = 0_u64;
    loop {
        match requests.recv_timeout(EVENT_POLL_INTERVAL) {
            Ok(MpvWorkerRequest::Command {
                command,
                event_boundary,
                response,
            }) => {
                request_id += 1;
                let result =
                    execute_mpv_command(&mut stream, &mut reader, &events, request_id, command);
                if result.is_ok()
                    && let Some(load_revision) = event_boundary
                {
                    send_mpv_event(
                        &events,
                        MpvEvent::LoadBoundary(load_revision),
                        "publishing load event boundary",
                    );
                }
                if response.send(result).is_err() {
                    eprintln!(
                        "Pinned mpv command response could not be delivered for request {request_id}: receiver closed."
                    );
                }
            }
            Ok(MpvWorkerRequest::Shutdown) => break,
            Err(RecvTimeoutError::Timeout) => drain_mpv_event(&mut reader, &events),
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("Pinned mpv worker stopped because its request channel disconnected.");
                break;
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                send_mpv_event(
                    &events,
                    MpvEvent::ExitedUnexpectedly(format!(
                        "Pinned mpv exited unexpectedly with {status}."
                    )),
                    "reporting unexpected process exit",
                );
                return;
            }
            Ok(None) => {}
            Err(error) => {
                send_mpv_event(
                    &events,
                    MpvEvent::ExitedUnexpectedly(format!(
                        "Pinned mpv status could not be read: {error}"
                    )),
                    "reporting process status failure",
                );
                break;
            }
        }
    }
    if let Err(error) = write_mpv_request(&mut stream, 0, json!(["quit"])) {
        eprintln!("Pinned mpv graceful Quit request failed during worker shutdown: {error}");
    }
    terminate_mpv(&mut child);
}

fn execute_mpv_command(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    events: &Sender<MpvEvent>,
    request_id: u64,
    command: Value,
) -> Result<Value, String> {
    write_mpv_request(stream, request_id, command)?;
    let deadline = Instant::now() + MPV_COMMAND_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(result) = read_mpv_message(reader, events, request_id)? {
            return result;
        }
    }
    Err("Pinned mpv IPC response timed out.".to_owned())
}

fn write_mpv_request(
    stream: &mut UnixStream,
    request_id: u64,
    command: Value,
) -> Result<(), String> {
    let message = json!({ "command": command, "request_id": request_id });
    serde_json::to_writer(&mut *stream, &message)
        .map_err(|error| format!("Pinned mpv command could not be encoded: {error}"))?;
    stream
        .write_all(b"\n")
        .map_err(|error| format!("Pinned mpv IPC write failed: {error}"))
}

fn read_mpv_message(
    reader: &mut BufReader<UnixStream>,
    events: &Sender<MpvEvent>,
    request_id: u64,
) -> Result<Option<Result<Value, String>>, String> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Err("Pinned mpv closed its private IPC connection.".to_owned()),
        Ok(_) => {
            let message: Value = serde_json::from_str(&line)
                .map_err(|error| format!("Pinned mpv returned invalid JSON IPC: {error}"))?;
            if message.get("request_id").and_then(Value::as_u64) == Some(request_id) {
                return Ok(Some(mpv_response_result(&message)));
            }
            emit_mpv_event(events, &message);
            Ok(None)
        }
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            Ok(None)
        }
        Err(error) => Err(format!("Pinned mpv IPC read failed: {error}")),
    }
}

fn mpv_response_result(message: &Value) -> Result<Value, String> {
    match message.get("error").and_then(Value::as_str) {
        Some("success") => Ok(message.get("data").cloned().unwrap_or(Value::Null)),
        Some(error) => Err(format!("Pinned mpv rejected command: {error}")),
        None => Err("Pinned mpv returned a command response without status.".to_owned()),
    }
}

fn drain_mpv_event(reader: &mut BufReader<UnixStream>, events: &Sender<MpvEvent>) {
    if let Err(error) = read_mpv_message(reader, events, u64::MAX) {
        report_native_playback_error(&format!("mpv event drain failed: {error}"));
        send_mpv_event(
            events,
            MpvEvent::ExitedUnexpectedly(error),
            "reporting IPC event drain failure",
        );
    }
}

fn emit_mpv_event(events: &Sender<MpvEvent>, message: &Value) {
    let event = match message.get("event").and_then(Value::as_str) {
        Some("property-change") => property_event(message),
        Some("end-file") if message.get("reason").and_then(Value::as_str) == Some("eof") => {
            Some(MpvEvent::Ended)
        }
        Some("end-file") if message.get("reason").and_then(Value::as_str) == Some("error") => {
            let detail = message
                .get("file_error")
                .and_then(Value::as_str)
                .unwrap_or("media could not be decoded");
            Some(MpvEvent::Error(format!(
                "Native playback failed: {detail}."
            )))
        }
        _ => None,
    };
    if let Some(event) = event {
        send_mpv_event(events, event, "publishing native playback event");
    }
}

fn send_mpv_event(events: &Sender<MpvEvent>, event: MpvEvent, context: &str) {
    if events.send(event).is_err() {
        eprintln!("Pinned mpv event delivery failed while {context}: receiver closed.");
    }
}

fn property_event(message: &Value) -> Option<MpvEvent> {
    match message.get("name").and_then(Value::as_str) {
        Some("time-pos") => message
            .get("data")
            .and_then(Value::as_f64)
            .map(MpvEvent::Time),
        Some("duration") => message
            .get("data")
            .and_then(Value::as_f64)
            .map(MpvEvent::Duration),
        Some("demuxer-cache-time") => message
            .get("data")
            .and_then(Value::as_f64)
            .map(MpvEvent::BufferedEnd),
        Some("pause") => message
            .get("data")
            .and_then(Value::as_bool)
            .map(MpvEvent::Paused),
        Some("audio-params") => message
            .get("data")
            .filter(|value| value.is_object())
            .map(ObservedMpvProperties::from_audio_params)
            .map(MpvEvent::Decoder),
        _ => None,
    }
}

fn terminate_mpv(child: &mut Child) {
    let deadline = Instant::now() + MPV_SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => {
                eprintln!("Pinned mpv status check failed during shutdown: {error}");
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    if let Err(error) = child.kill()
        && error.kind() != ErrorKind::InvalidInput
    {
        eprintln!("Pinned mpv forced shutdown failed: {error}");
    }
    if let Err(error) = child.wait() {
        eprintln!("Pinned mpv process reap failed after shutdown: {error}");
    }
}

type StateListener = Arc<dyn Fn(PlaybackSessionState) + Send + Sync>;
type ProcessStarter =
    Arc<dyn Fn() -> Result<(Box<dyn MpvProcessAdapter>, Receiver<MpvEvent>), String> + Send + Sync>;

struct PlaybackControllerSetup {
    lifecycle: Arc<Mutex<PlaybackLifecycle>>,
    starter: Option<ProcessStarter>,
    path_observer: Option<Arc<dyn PipeWireObserver>>,
    output_device_adapter: Option<Box<dyn AlsaOutputDeviceAdapter>>,
    adaptive_system_rate: Option<AdaptiveSystemRateController>,
}

impl PlaybackControllerSetup {
    fn new(lifecycle: Arc<Mutex<PlaybackLifecycle>>) -> Self {
        Self {
            lifecycle,
            starter: None,
            path_observer: None,
            output_device_adapter: None,
            adaptive_system_rate: None,
        }
    }
}

pub(crate) struct PlaybackController {
    process: Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: Arc<Mutex<PlaybackSessionState>>,
    output_devices: Option<Arc<Mutex<OutputDeviceCatalog>>>,
    adaptive_system_rate: Option<Arc<Mutex<AdaptiveSystemRateController>>>,
    queue: Arc<Mutex<PlaybackQueueContext>>,
    lifecycle: Arc<Mutex<PlaybackLifecycle>>,
    is_shutdown: Arc<AtomicBool>,
    command_revision: Arc<AtomicU64>,
    load_revision: Arc<AtomicU64>,
    published_load_revision: Arc<AtomicU64>,
    transition_fade_ms: Arc<AtomicU64>,
    event_thread: Option<JoinHandle<()>>,
    listener: StateListener,
}

impl PlaybackController {
    pub(crate) fn start_default_with_lifecycle(
        lifecycle: Arc<Mutex<PlaybackLifecycle>>,
        path_observer: Arc<dyn PipeWireObserver>,
        adaptive_system_rate: AdaptiveSystemRateController,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let starter: ProcessStarter = Arc::new(RealMpvProcess::start_default);
        let (process, events) = starter()?;
        Ok(Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                starter: Some(starter),
                path_observer: Some(path_observer),
                output_device_adapter: Some(Box::new(CommandAlsaOutputDeviceAdapter::new())),
                adaptive_system_rate: Some(adaptive_system_rate),
                ..PlaybackControllerSetup::new(lifecycle)
            },
            listener,
        ))
    }

    #[cfg(test)]
    pub(crate) fn start(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup::new(Arc::new(Mutex::new(PlaybackLifecycle::new()))),
            listener,
        )
    }

    #[cfg(test)]
    fn start_with_path_observer(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        path_observer: Arc<dyn PipeWireObserver>,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                path_observer: Some(path_observer),
                ..PlaybackControllerSetup::new(Arc::new(Mutex::new(PlaybackLifecycle::new())))
            },
            listener,
        )
    }

    #[cfg(test)]
    fn start_recoverable(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        lifecycle: Arc<Mutex<PlaybackLifecycle>>,
        starter: impl Fn() -> Result<(Box<dyn MpvProcessAdapter>, Receiver<MpvEvent>), String>
        + Send
        + Sync
        + 'static,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                starter: Some(Arc::new(starter)),
                ..PlaybackControllerSetup::new(lifecycle)
            },
            listener,
        )
    }

    #[cfg(test)]
    fn start_recoverable_with_adaptive_system_rate(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        lifecycle: Arc<Mutex<PlaybackLifecycle>>,
        starter: impl Fn() -> Result<(Box<dyn MpvProcessAdapter>, Receiver<MpvEvent>), String>
        + Send
        + Sync
        + 'static,
        adaptive_system_rate: AdaptiveSystemRateController,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                starter: Some(Arc::new(starter)),
                adaptive_system_rate: Some(adaptive_system_rate),
                ..PlaybackControllerSetup::new(lifecycle)
            },
            listener,
        )
    }

    #[cfg(test)]
    fn start_with_output_devices(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        output_devices: Box<dyn AlsaOutputDeviceAdapter>,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                output_device_adapter: Some(output_devices),
                ..PlaybackControllerSetup::new(Arc::new(Mutex::new(PlaybackLifecycle::new())))
            },
            listener,
        )
    }

    #[cfg(test)]
    fn start_with_output_devices_and_path_observer(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        path_observer: Arc<dyn PipeWireObserver>,
        output_devices: Box<dyn AlsaOutputDeviceAdapter>,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                path_observer: Some(path_observer),
                output_device_adapter: Some(output_devices),
                ..PlaybackControllerSetup::new(Arc::new(Mutex::new(PlaybackLifecycle::new())))
            },
            listener,
        )
    }

    #[cfg(test)]
    fn start_with_adaptive_system_rate(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        adaptive_system_rate: AdaptiveSystemRateController,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        Self::start_internal(
            process,
            events,
            PlaybackControllerSetup {
                adaptive_system_rate: Some(adaptive_system_rate),
                ..PlaybackControllerSetup::new(Arc::new(Mutex::new(PlaybackLifecycle::new())))
            },
            listener,
        )
    }

    fn start_internal(
        process: Box<dyn MpvProcessAdapter>,
        events: Receiver<MpvEvent>,
        setup: PlaybackControllerSetup,
        listener: impl Fn(PlaybackSessionState) + Send + Sync + 'static,
    ) -> Self {
        let PlaybackControllerSetup {
            lifecycle,
            starter,
            path_observer,
            output_device_adapter,
            adaptive_system_rate,
        } = setup;
        let process = Arc::new(Mutex::new(process));
        let state = Arc::new(Mutex::new(PlaybackSessionState::default()));
        let queue = Arc::new(Mutex::new(PlaybackQueueContext::default()));
        let is_shutdown = Arc::new(AtomicBool::new(false));
        let command_revision = Arc::new(AtomicU64::new(0));
        let load_revision = Arc::new(AtomicU64::new(0));
        let published_load_revision = Arc::new(AtomicU64::new(0));
        let listener: StateListener = Arc::new(listener);
        let output_devices = output_device_adapter
            .map(OutputDeviceCatalog::new)
            .map(Mutex::new)
            .map(Arc::new);
        let adaptive_system_rate = adaptive_system_rate.map(Mutex::new).map(Arc::new);
        let event_thread = spawn_event_thread(
            PlaybackEventContext {
                process: process.clone(),
                state: state.clone(),
                queue: queue.clone(),
                is_shutdown: is_shutdown.clone(),
                command_revision: command_revision.clone(),
                load_revision: load_revision.clone(),
                published_load_revision: published_load_revision.clone(),
                listener: listener.clone(),
                lifecycle: lifecycle.clone(),
                starter,
                path_observer,
                output_devices: output_devices.clone(),
                adaptive_system_rate: adaptive_system_rate.clone(),
            },
            events,
        );
        Self {
            process,
            state,
            output_devices,
            adaptive_system_rate,
            queue,
            lifecycle,
            is_shutdown,
            command_revision,
            load_revision,
            published_load_revision,
            transition_fade_ms: Arc::new(AtomicU64::new(0)),
            event_thread: Some(event_thread),
            listener,
        }
    }

    pub(crate) fn state(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))
    }

    pub(crate) fn shutdown(&self) -> Result<(), PlaybackCommandError> {
        self.disable_adaptive_system_rate()?;
        self.shutdown_player()
    }

    fn shutdown_player(&self) -> Result<(), PlaybackCommandError> {
        self.advance_command_revision();
        self.is_shutdown.store(true, Ordering::Release);
        self.process
            .lock()
            .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?
            .shutdown();
        Ok(())
    }

    pub(crate) fn restore_paused(
        &self,
        snapshot: &PlaybackSessionSnapshot,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.advance_command_revision();
        let playback_rate = self.state()?.playback_rate;
        let action = self.with_process(|process| {
            apply_snapshot_to_process(process, snapshot, false, playback_rate)
        });
        if let Err(error) = action {
            return self.fail_native(error);
        }
        let state = self.update(|state| restore_snapshot_state(state, snapshot))?;
        self.mark_player_stable()?;
        Ok(state)
    }

    pub(crate) fn play(
        &self,
        source: Option<Value>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.advance_command_revision();
        let current = self.state()?;
        let next_source = source.clone().or(current.source.clone());
        let Some(next_source) = next_source else {
            return Ok(current);
        };
        let is_new_source = source.is_some();
        let should_fade_in = !is_new_source
            && current.status == PlaybackStatus::Paused
            && self.transition_fade_ms.load(Ordering::Acquire) > 0;
        if should_fade_in && let Err(error) = self.with_process(|process| process.set_volume(0.0)) {
            report_native_playback_error(&format!("Transition fade-in setup failed: {error}"));
        }
        if is_new_source && let Err(error) = self.apply_adaptive_source_rate(&next_source) {
            self.reset_adaptive_after_failure("source rate setup");
            return self.fail_native(error);
        }
        let load_revision = match self.start_playback(&next_source, is_new_source) {
            Ok(load_revision) => load_revision,
            Err(error) => {
                self.reset_adaptive_after_failure("mpv playback setup");
                return self.fail_native(error);
            }
        };
        let state =
            self.update(|state| update_playing_state(state, next_source.clone(), is_new_source))?;
        if let Some(load_revision) = load_revision {
            self.published_load_revision
                .store(load_revision, Ordering::Release);
        }
        self.align_queue_to_source(&next_source)?;
        self.mark_player_stable()?;
        if should_fade_in {
            self.fade_in();
        }
        Ok(state)
    }

    fn start_playback(&self, source: &Value, should_load: bool) -> Result<Option<u64>, String> {
        if !should_load {
            return self
                .with_process(|process| process.set_paused(false))
                .map(|_| None);
        }
        let url = playback_url(source).map_err(|error| error.message)?;
        let load_revision = self.next_load_revision();
        self.with_process(|process| {
            process.load_with_event_boundary(&url, load_revision)?;
            process.set_paused(false)
        })
        .map(|_| Some(load_revision))
    }

    fn apply_adaptive_source_rate(&self, source: &Value) -> Result<(), String> {
        if self.state().map_err(|error| error.message)?.output_mode
            != OutputMode::AdaptiveSystemRate
        {
            return Ok(());
        }
        let rate_hz = SourceObservation::from_playback_source(source)
            .format
            .sample_rate_hz;
        let adaptive = self
            .adaptive_system_rate
            .as_ref()
            .ok_or_else(|| "Adaptive System Rate is unavailable on this system.".to_owned())?;
        adaptive
            .lock()
            .map_err(|_| "Adaptive System Rate state is unavailable.".to_owned())?
            .apply_source_sample_rate(rate_hz)
    }

    fn reset_adaptive_after_failure(&self, context: &str) {
        let Some(adaptive) = self.adaptive_system_rate.as_ref() else {
            return;
        };
        match adaptive.lock() {
            Ok(mut adaptive) => {
                if let Err(error) = adaptive.reset_for_recovery() {
                    report_native_playback_error(&format!(
                        "Adaptive System Rate cleanup failed after {context}: {error}"
                    ));
                }
            }
            Err(_) => report_native_playback_error(&format!(
                "Adaptive System Rate cleanup failed after {context}: state is unavailable"
            )),
        }
    }

    fn disable_adaptive_system_rate(&self) -> Result<(), PlaybackCommandError> {
        let Some(adaptive) = self.adaptive_system_rate.as_ref() else {
            return Ok(());
        };
        let result = adaptive
            .lock()
            .map_err(|_| PlaybackCommandError::new("Adaptive System Rate state is unavailable."))?
            .disable();
        result.map_err(|error| {
            report_native_playback_error(&format!("Adaptive System Rate disable failed: {error}"));
            PlaybackCommandError::new(
                "Adaptive System Rate could not be disabled. Restart the Desktop Client.",
            )
        })
    }

    pub(crate) fn pause(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.advance_command_revision();
        if self.state()?.source.is_none() {
            return self.state();
        }
        self.fade_out();
        let paused = self.with_process(|process| process.set_paused(true));
        // Leave mpv at the real level so the next play starts audible even
        // when the fade-in is later disabled.
        self.restore_volume();
        if let Err(error) = paused {
            return self.fail_native(error);
        }
        self.update(|state| state.status = PlaybackStatus::Paused)
    }

    pub(crate) fn stop(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.advance_command_revision();
        self.fade_out();
        let stopped = self.with_process(|process| process.stop());
        self.restore_volume();
        if let Err(error) = stopped {
            return self.fail_native(error);
        }
        self.update(|state| {
            state.source = None;
            state.processing.effective_replay_gain_mode =
                effective_replay_gain_mode(state.processing.replay_gain_mode, None);
            state.status = PlaybackStatus::Idle;
            state.current_time = 0.0;
            state.buffered_end = None;
            state.duration = 0.0;
            state.error = None;
            state.telemetry = telemetry_for_output_mode(
                state,
                SourceObservation::unknown(),
                ObservedMpvProperties::unknown(),
                None,
            );
        })
    }

    pub(crate) fn toggle_play(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        if self.state()?.status.is_active() {
            self.pause()
        } else {
            self.play(None)
        }
    }

    pub(crate) fn seek(&self, seconds: f64) -> Result<PlaybackSessionState, PlaybackCommandError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return self.fail("Playback seek position must be a non-negative finite number.");
        }
        if let Err(error) = self.with_process(|process| process.seek(seconds)) {
            return self.fail_native(error);
        }
        self.update(|state| state.current_time = seconds)
    }

    pub(crate) fn apply_processing(
        &self,
        mut processing: ProcessingState,
        configuration: &MpvProcessingConfiguration,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let applied = match self.with_process(|process| process.apply_processing(configuration)) {
            Ok(applied) => applied,
            Err(error) => return self.fail_native(error),
        };
        processing.software_volume = (applied.volume_percent / 100.0).clamp(0.0, 1.0);
        processing.replay_gain_mode = applied.replay_gain_mode;
        processing.equalizer.is_enabled = !applied.audio_filters.is_empty();
        processing.effective_audio_filters = applied.audio_filters;
        self.update(|state| {
            processing.effective_replay_gain_mode =
                effective_replay_gain_mode(applied.replay_gain_mode, state.source.as_ref());
            state.volume = processing.software_volume;
            state.processing = processing.clone();
            state.telemetry.processing = processing_observation(&processing);
        })
    }

    pub(crate) fn enable_adaptive_system_rate(
        &self,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let source_rate_hz = self.adaptive_source_rate_hz()?;
        self.enable_adaptive_controller()?;
        if let Err(error) = self.configure_system_output() {
            return self.fail_adaptive_setup(error);
        }
        if let Err(error) = self.apply_adaptive_rate_hz(source_rate_hz) {
            return self.fail_adaptive_setup(error);
        }
        self.update(publish_adaptive_system_output)
    }

    fn adaptive_source_rate_hz(&self) -> Result<Option<u32>, PlaybackCommandError> {
        Ok(self
            .state()?
            .source
            .as_ref()
            .map(SourceObservation::from_playback_source)
            .and_then(|source| source.format.sample_rate_hz))
    }

    fn enable_adaptive_controller(&self) -> Result<(), PlaybackCommandError> {
        let result = self
            .adaptive_controller()?
            .lock()
            .map_err(|_| PlaybackCommandError::new("Adaptive System Rate state is unavailable."))?
            .enable();
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                report_native_playback_error(&format!(
                    "Adaptive System Rate enable preparation failed: {error}"
                ));
                Err(PlaybackCommandError::new(
                    "Adaptive System Rate could not be enabled because its cleanup safeguard could not be saved. Check Desktop Client storage permissions and try again.",
                ))
            }
        }
    }

    fn apply_adaptive_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
        self.adaptive_controller()
            .map_err(|error| error.message)?
            .lock()
            .map_err(|_| "Adaptive System Rate state is unavailable.".to_owned())?
            .apply_source_sample_rate(rate_hz)
    }

    fn adaptive_controller(
        &self,
    ) -> Result<&Arc<Mutex<AdaptiveSystemRateController>>, PlaybackCommandError> {
        self.adaptive_system_rate.as_ref().ok_or_else(|| {
            PlaybackCommandError::new("Adaptive System Rate is unavailable on this system.")
        })
    }

    fn fail_adaptive_setup(
        &self,
        error: String,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.disable_adaptive_system_rate()?;
        self.fail_native(error)
    }

    pub(crate) fn refresh_output_devices(
        &self,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let (devices, disconnected) = self.refresh_output_catalog()?;
        let Some(disconnected) = disconnected else {
            return self.publish_available_output_devices(devices);
        };
        if self.state()?.output_mode != OutputMode::DirectAlsa {
            return self.update(|state| state.available_output_devices = devices.clone());
        }
        self.pause_after_output_failure();
        self.update(|state| {
            state.available_output_devices = devices.clone();
            if state.source.is_some() {
                state.status = PlaybackStatus::Paused;
            }
            state.output_device_issue = Some(disconnected_device_issue(&disconnected));
        })
    }

    fn refresh_output_catalog(
        &self,
    ) -> Result<(Vec<OutputDevice>, Option<OutputDevice>), PlaybackCommandError> {
        let Some(catalog) = self.output_devices.as_ref() else {
            return Err(PlaybackCommandError::new(
                "Raw ALSA hardware enumeration is unavailable.",
            ));
        };
        let mut catalog = catalog
            .lock()
            .map_err(|_| PlaybackCommandError::new("Output Device state is unavailable."))?;
        match catalog.refresh() {
            Ok(devices) => Ok((devices, None)),
            Err(OutputDeviceError::SelectedDeviceDisconnected(device)) => {
                Ok((catalog.devices().to_vec(), Some(device)))
            }
            Err(error) => Err(PlaybackCommandError::new(output_device_error_message(
                error,
            ))),
        }
    }

    fn publish_available_output_devices(
        &self,
        devices: Vec<OutputDevice>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.update(|state| {
            state.available_output_devices = devices.clone();
            if state
                .output_device_issue
                .as_ref()
                .is_some_and(|issue| issue.code == OutputDeviceIssueCode::Disconnected)
            {
                state.output_device_issue = None;
            }
        })
    }

    pub(crate) fn select_direct_alsa_output(
        &self,
        device_id: &str,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let previous = self.state()?;
        let device = self.select_output_device(device_id)?;
        self.disable_adaptive_system_rate()?;
        let negotiated_format = match self.configure_direct_output(&device) {
            Ok(format) => format,
            Err(error) => return self.handle_direct_output_failure(previous, device, error),
        };
        let negotiated_format = match self
            .observe_direct_output_if_ready(negotiated_format, previous.source.is_some())
        {
            Ok(format) => format,
            Err(error) => return self.handle_direct_output_failure(previous, device, error),
        };
        self.publish_direct_output(device, negotiated_format)
    }

    fn observe_direct_output_if_ready(
        &self,
        configured_format: Option<AudioFormatObservation>,
        has_source: bool,
    ) -> Result<Option<AudioFormatObservation>, String> {
        if configured_format.is_some() || !has_source {
            return Ok(configured_format);
        }
        let deadline = Instant::now() + DIRECT_OUTPUT_READY_TIMEOUT;
        let mut last_error = None;
        while Instant::now() < deadline {
            match self.with_process(|process| process.observe_output_format()) {
                Ok(Some(observed)) => return Ok(Some(observed)),
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
            thread::sleep(DIRECT_OUTPUT_RETRY_INTERVAL);
        }
        Err(last_error.unwrap_or_else(|| {
            "Direct ALSA Output did not report an active audio format.".to_owned()
        }))
    }

    fn select_output_device(&self, device_id: &str) -> Result<OutputDevice, PlaybackCommandError> {
        let Some(catalog) = self.output_devices.as_ref() else {
            return Err(PlaybackCommandError::new(
                "Direct ALSA Output is unavailable.",
            ));
        };
        let result = catalog
            .lock()
            .map_err(|_| PlaybackCommandError::new("Output Device state is unavailable."))?
            .select(device_id);
        result.map_err(|error| {
            report_native_playback_error(&format!("Output Device selection failed: {error:?}"));
            PlaybackCommandError::new(output_device_public_message(&error))
        })
    }

    fn configure_direct_output(
        &self,
        device: &OutputDevice,
    ) -> Result<Option<AudioFormatObservation>, String> {
        self.with_process(|process| {
            process.configure_output(&MpvOutputConfiguration::DirectAlsa {
                device_id: device.id.clone(),
            })
        })
    }

    fn publish_direct_output(
        &self,
        device: OutputDevice,
        negotiated_format: Option<AudioFormatObservation>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.update(|state| {
            state.output_mode = OutputMode::DirectAlsa;
            state.selected_output_device = Some(device.clone());
            state.output_device_issue = None;
            state.telemetry = PlaybackTelemetry::native_direct_alsa_output(
                source_observation(state.source.as_ref()),
                state.telemetry.decoder.clone(),
                &device.name,
                negotiated_format.clone(),
                processing_observation(&state.processing),
            );
        })
    }

    fn handle_direct_output_failure(
        &self,
        previous: PlaybackSessionState,
        attempted: OutputDevice,
        error: String,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        report_native_playback_error(&format!(
            "Direct ALSA Output could not open '{}': {error}",
            attempted.id
        ));
        let restored_mode = self.restore_output_route(&previous);
        self.pause_after_output_failure();
        self.update(|state| {
            state.output_mode = restored_mode;
            state.selected_output_device = if restored_mode == OutputMode::DirectAlsa {
                previous.selected_output_device.clone()
            } else {
                Some(attempted.clone())
            };
            if state.source.is_some() {
                state.status = PlaybackStatus::Paused;
            }
            state.output_device_issue = Some(busy_or_unsupported_device_issue(&attempted));
            reset_output_telemetry(state);
        })
    }

    fn restore_output_route(&self, previous: &PlaybackSessionState) -> OutputMode {
        let result = match previous.output_mode {
            OutputMode::DirectAlsa => previous
                .selected_output_device
                .as_ref()
                .ok_or_else(|| "Previous Direct ALSA Output Device is unavailable.".to_owned())
                .and_then(|device| self.configure_direct_output(device).map(|_| ())),
            OutputMode::AdaptiveSystemRate => {
                return self.restore_adaptive_output_route();
            }
            OutputMode::System => self.configure_system_output(),
        };
        if let Err(error) = result {
            report_native_playback_error(&format!(
                "Previous Output Mode could not be restored: {error}"
            ));
            return self.restore_safe_system_output();
        }
        previous.output_mode
    }

    fn restore_adaptive_output_route(&self) -> OutputMode {
        match self.enable_adaptive_system_rate() {
            Ok(_) => OutputMode::AdaptiveSystemRate,
            Err(error) => {
                report_native_playback_error(&format!(
                    "Adaptive System Rate rollback failed: {}",
                    error.message
                ));
                self.restore_safe_system_output()
            }
        }
    }

    fn restore_safe_system_output(&self) -> OutputMode {
        if let Err(error) = self.configure_system_output() {
            report_native_playback_error(&format!("Safe System Output rollback failed: {error}"));
        }
        OutputMode::System
    }

    fn configure_system_output(&self) -> Result<(), String> {
        self.with_process(|process| {
            process
                .configure_output(&MpvOutputConfiguration::System)
                .map(|_| ())
        })
    }

    fn pause_after_output_failure(&self) {
        if let Err(error) = self.with_process(|process| process.set_paused(true)) {
            report_native_playback_error(&format!(
                "Direct ALSA Output failure could not pause playback: {error}"
            ));
        }
    }

    pub(crate) fn fallback_to_system_output(
        &self,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let previous_mode = self.state()?.output_mode;
        self.disable_adaptive_system_rate()?;
        if let Err(error) = self.configure_system_output() {
            self.restore_adaptive_after_system_failure(previous_mode);
            return self.fail_native(error);
        }
        self.update(publish_system_output)
    }

    fn restore_adaptive_after_system_failure(&self, previous_mode: OutputMode) {
        if previous_mode != OutputMode::AdaptiveSystemRate {
            return;
        }
        if let Err(error) = self.enable_adaptive_system_rate() {
            report_native_playback_error(&format!(
                "Adaptive System Rate rollback failed after System Output error: {}",
                error.message
            ));
        }
    }

    pub(crate) fn toggle_shuffle(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let state = self.update(|state| state.shuffle_enabled = !state.shuffle_enabled)?;
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?;
        if state.shuffle_enabled {
            queue.shuffle().map_err(PlaybackCommandError::new)?;
        } else {
            queue.reset_order();
        }
        Ok(state)
    }

    pub(crate) fn sync_queue_context(
        &self,
        sources: Vec<Value>,
        current_index: Option<usize>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let state = self.state()?;
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?;
        queue
            .sync(sources, current_index)
            .map_err(PlaybackCommandError::new)?;
        if let Some(source) = state
            .source
            .as_ref()
            .filter(|source| source["type"] == "track")
        {
            // A reattached renderer may not know the native session's current Queue position.
            queue
                .align_to_source(source)
                .map_err(PlaybackCommandError::new)?;
        }
        if state.shuffle_enabled {
            queue.shuffle().map_err(PlaybackCommandError::new)?;
        }
        drop(queue);
        self.state()
    }

    pub(crate) fn previous(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.navigate(QueueDirection::Previous)
    }

    pub(crate) fn next(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.navigate(QueueDirection::Next)
    }

    #[cfg(test)]
    fn queue_sources(&self) -> Result<Vec<Value>, PlaybackCommandError> {
        self.queue
            .lock()
            .map(|queue| queue.sources())
            .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))
    }

    pub(crate) fn cycle_repeat_mode(&self) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.update(|state| {
            state.repeat_mode = match state.repeat_mode {
                RepeatMode::Off => RepeatMode::Once,
                RepeatMode::Once => RepeatMode::Loop,
                RepeatMode::Loop => RepeatMode::Off,
            };
        })
    }

    fn with_process<T>(
        &self,
        action: impl FnOnce(&dyn MpvProcessAdapter) -> Result<T, String>,
    ) -> Result<T, String> {
        let process = self
            .process
            .lock()
            .map_err(|_| "Native mpv process is unavailable.".to_owned())?;
        action(process.as_ref())
    }

    fn advance_command_revision(&self) {
        self.command_revision.fetch_add(1, Ordering::AcqRel);
    }

    fn next_load_revision(&self) -> u64 {
        self.load_revision.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn update(
        &self,
        change: impl FnOnce(&mut PlaybackSessionState),
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let next = update_shared_state(&self.state, change)?;
        (self.listener)(next.clone());
        Ok(next)
    }

    fn mark_player_stable(&self) -> Result<(), PlaybackCommandError> {
        self.lifecycle
            .lock()
            .map_err(|_| PlaybackCommandError::new("Playback lifecycle state is unavailable."))?
            .player_stabilized();
        Ok(())
    }

    fn navigate(
        &self,
        direction: QueueDirection,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let should_wrap = self.state()?.repeat_mode == RepeatMode::Loop;
        let source = self
            .queue
            .lock()
            .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?
            .adjacent_source(direction, should_wrap);
        match source {
            Some(source) => {
                self.fade_out();
                let result = self.play(Some(source));
                if result.is_ok() {
                    self.fade_in();
                } else {
                    self.restore_volume();
                }
                result
            }
            None => self.state(),
        }
    }

    /// Set mpv's playback speed. mpv keeps pitch by default, so this is a
    /// tempo change rather than a pitch shift.
    pub(crate) fn set_playback_rate(
        &self,
        rate: f64,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        if !rate.is_finite() || !(MIN_PLAYBACK_RATE..=MAX_PLAYBACK_RATE).contains(&rate) {
            return self.fail(format!(
                "Playback speed must be between {MIN_PLAYBACK_RATE} and {MAX_PLAYBACK_RATE}."
            ));
        }
        if let Err(error) = self.with_process(|process| process.set_speed(rate)) {
            return self.fail_native(error);
        }
        self.update(|state| state.playback_rate = rate)
    }

    pub(crate) fn set_stop_after_current(
        &self,
        enabled: bool,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.update(|state| state.stop_after_current = enabled)
    }

    /// Length of the volume ramp applied around user-initiated transitions.
    /// Natural gapless boundaries are never faded.
    pub(crate) fn set_transition_fade_ms(
        &self,
        milliseconds: u64,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        self.transition_fade_ms
            .store(milliseconds.min(MAX_TRANSITION_FADE_MS), Ordering::Release);
        self.state()
    }

    #[cfg(test)]
    fn transition_fade_ms(&self) -> u64 {
        self.transition_fade_ms.load(Ordering::Acquire)
    }

    fn fade_out(&self) {
        let Some((volume, fade_ms)) = self.active_fade() else {
            return;
        };
        self.ramp_volume(volume, 0.0, fade_ms);
    }

    fn fade_in(&self) {
        let Some((volume, fade_ms)) = self.active_fade() else {
            return;
        };
        self.ramp_volume(0.0, volume, fade_ms);
    }

    fn restore_volume(&self) {
        let fade_ms = self.transition_fade_ms.load(Ordering::Acquire);
        if fade_ms == 0 {
            return;
        }
        let Ok(state) = self.state() else {
            return;
        };
        if let Err(error) = self.with_process(|process| process.set_volume(state.volume)) {
            report_native_playback_error(&format!(
                "Transition fade volume restore failed: {error}"
            ));
        }
    }

    /// Volume and fade length when a fade should run: the fade is enabled
    /// and something is actually playing.
    fn active_fade(&self) -> Option<(f64, u64)> {
        let fade_ms = self.transition_fade_ms.load(Ordering::Acquire);
        if fade_ms == 0 {
            return None;
        }
        let state = self.state().ok()?;
        if !state.status.is_active() {
            return None;
        }
        Some((state.volume, fade_ms))
    }

    fn ramp_volume(&self, from: f64, to: f64, fade_ms: u64) {
        let steps = (fade_ms / TRANSITION_FADE_STEP.as_millis() as u64).max(1);
        for step in 1..=steps {
            let volume = from + (to - from) * (step as f64 / steps as f64);
            if let Err(error) = self.with_process(|process| process.set_volume(volume)) {
                report_native_playback_error(&format!("Transition fade step failed: {error}"));
                return;
            }
            if step < steps {
                thread::sleep(TRANSITION_FADE_STEP);
            }
        }
    }

    fn align_queue_to_source(&self, source: &Value) -> Result<(), PlaybackCommandError> {
        self.queue
            .lock()
            .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?
            .align_to_source(source)
            .map_err(PlaybackCommandError::new)
    }

    fn fail(
        &self,
        message: impl Into<String>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        let message = message.into();
        if let Err(error) = self.update(|state| set_error_state(state, message.clone())) {
            eprintln!(
                "Playback failure state could not be published for '{message}': {}",
                error.message
            );
        }
        Err(PlaybackCommandError::new(message))
    }

    fn fail_native(
        &self,
        detail: impl Into<String>,
    ) -> Result<PlaybackSessionState, PlaybackCommandError> {
        report_native_playback_error(&detail.into());
        self.fail(PLAYBACK_FAILED_MESSAGE)
    }
}

fn apply_snapshot_to_process(
    process: &dyn MpvProcessAdapter,
    snapshot: &PlaybackSessionSnapshot,
    should_resume: bool,
    playback_rate: f64,
) -> Result<(), String> {
    process.set_paused(true)?;
    process.set_volume(snapshot.volume())?;
    process.set_speed(playback_rate)?;
    if let Some(source) = snapshot.source() {
        let url = playback_url(source).map_err(|error| error.message)?;
        process.load_at(&url, snapshot.playhead_seconds())?;
        if should_resume {
            process.set_paused(false)?;
        }
    }
    Ok(())
}

fn restore_snapshot_state(state: &mut PlaybackSessionState, snapshot: &PlaybackSessionSnapshot) {
    state.source = snapshot.source().cloned();
    state.status = if state.source.is_some() {
        PlaybackStatus::Paused
    } else {
        PlaybackStatus::Idle
    };
    state.current_time = snapshot.playhead_seconds();
    state.duration = state.source.as_ref().map_or(0.0, track_duration);
    state.volume = snapshot.volume();
    state.shuffle_enabled = snapshot.is_shuffle_enabled();
    state.repeat_mode = match snapshot.repeat_mode() {
        "once" => RepeatMode::Once,
        "loop" => RepeatMode::Loop,
        _ => RepeatMode::Off,
    };
    state.processing.effective_replay_gain_mode =
        effective_replay_gain_mode(state.processing.replay_gain_mode, state.source.as_ref());
    state.telemetry = telemetry_for_output_mode(
        state,
        state
            .source
            .as_ref()
            .map(SourceObservation::from_playback_source)
            .unwrap_or_else(SourceObservation::unknown),
        ObservedMpvProperties::unknown(),
        None,
    );
    state.error = None;
}

impl Drop for PlaybackController {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            report_native_playback_error(&format!(
                "Native playback cleanup failed during teardown: {}",
                error.message
            ));
            if let Err(error) = self.shutdown_player() {
                report_native_playback_error(&format!(
                    "mpv cleanup failed during teardown: {}",
                    error.message
                ));
            }
        }
        if let Some(event_thread) = self.event_thread.take()
            && event_thread.join().is_err()
        {
            eprintln!("Native playback event thread shutdown failed: thread panicked.");
        }
    }
}

fn spawn_event_thread(
    context: PlaybackEventContext,
    mut events: Receiver<MpvEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || run_playback_event_loop(context, &mut events))
}

struct PlaybackEventContext {
    process: Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: Arc<Mutex<PlaybackSessionState>>,
    queue: Arc<Mutex<PlaybackQueueContext>>,
    is_shutdown: Arc<AtomicBool>,
    command_revision: Arc<AtomicU64>,
    load_revision: Arc<AtomicU64>,
    published_load_revision: Arc<AtomicU64>,
    listener: StateListener,
    lifecycle: Arc<Mutex<PlaybackLifecycle>>,
    starter: Option<ProcessStarter>,
    path_observer: Option<Arc<dyn PipeWireObserver>>,
    output_devices: Option<Arc<Mutex<OutputDeviceCatalog>>>,
    adaptive_system_rate: Option<Arc<Mutex<AdaptiveSystemRateController>>>,
}

#[derive(Default)]
struct LiveReconnectState {
    armed_source: Option<Value>,
    command_revision: u64,
    attempt: usize,
    retry_at: Option<Instant>,
    readiness_deadline: Option<Instant>,
    stability_deadline: Option<Instant>,
    accepted_load_revision: Option<u64>,
}

fn run_playback_event_loop(context: PlaybackEventContext, events: &mut Receiver<MpvEvent>) {
    let mut last_path_refresh = Instant::now();
    let mut last_output_device_refresh = Instant::now();
    let mut live_reconnect = LiveReconnectState::default();
    while !context.is_shutdown.load(Ordering::Acquire) {
        if !receive_playback_event(
            &context,
            events,
            &mut last_path_refresh,
            &mut live_reconnect,
        ) {
            break;
        }
        poll_live_reconnect(&context, &mut live_reconnect);
        if last_output_device_refresh.elapsed() >= OUTPUT_DEVICE_REFRESH_INTERVAL {
            refresh_direct_output_device(
                &context.process,
                &context.state,
                &context.listener,
                context.output_devices.as_deref(),
            );
            last_output_device_refresh = Instant::now();
        }
    }
}

fn receive_playback_event(
    context: &PlaybackEventContext,
    events: &mut Receiver<MpvEvent>,
    last_path_refresh: &mut Instant,
    live_reconnect: &mut LiveReconnectState,
) -> bool {
    match events.recv_timeout(EVENT_POLL_INTERVAL) {
        Ok(event) => {
            dispatch_playback_event(context, events, event, last_path_refresh, live_reconnect)
        }
        Err(RecvTimeoutError::Timeout) => true,
        Err(RecvTimeoutError::Disconnected) => {
            if !context.is_shutdown.load(Ordering::Acquire) {
                report_native_playback_error("playback event channel disconnected unexpectedly");
            }
            false
        }
    }
}

fn dispatch_playback_event(
    context: &PlaybackEventContext,
    events: &mut Receiver<MpvEvent>,
    event: MpvEvent,
    last_path_refresh: &mut Instant,
    live_reconnect: &mut LiveReconnectState,
) -> bool {
    match event {
        MpvEvent::ExitedUnexpectedly(message) => {
            if let Some(next_events) = recover_after_player_exit(
                &context.process,
                &context.state,
                &context.lifecycle,
                context.starter.as_ref(),
                &context.listener,
                context.adaptive_system_rate.as_deref(),
                message,
            ) {
                *events = next_events;
            }
        }
        event => dispatch_regular_playback_event(context, event, last_path_refresh, live_reconnect),
    }
    true
}

fn dispatch_regular_playback_event(
    context: &PlaybackEventContext,
    event: MpvEvent,
    last_path_refresh: &mut Instant,
    live_reconnect: &mut LiveReconnectState,
) {
    if let MpvEvent::LoadBoundary(load_revision) = &event {
        handle_load_boundary(context, live_reconnect, *load_revision);
        return;
    }
    if is_stable_playback_event(&event) {
        handle_live_stable_event(context, live_reconnect);
    }
    if is_reconnectable_live_interruption(&event) && start_live_reconnect(context, live_reconnect) {
        return;
    }
    if let MpvEvent::Decoder(decoder) = &event {
        apply_adaptive_decoder_rate(
            &context.state,
            &context.listener,
            context.adaptive_system_rate.as_deref(),
            decoder.format.sample_rate_hz,
        );
    }
    if process_mpv_event(context, event, *last_path_refresh) {
        *last_path_refresh = Instant::now();
    }
}

fn handle_load_boundary(
    context: &PlaybackEventContext,
    live_reconnect: &mut LiveReconnectState,
    load_revision: u64,
) {
    if context.load_revision.load(Ordering::Acquire) == load_revision {
        if live_reconnect.armed_source.is_some()
            && context.command_revision.load(Ordering::Acquire) != live_reconnect.command_revision
        {
            *live_reconnect = LiveReconnectState::default();
        }
        live_reconnect.accepted_load_revision = Some(load_revision);
    }
}

fn handle_live_stable_event(
    context: &PlaybackEventContext,
    live_reconnect: &mut LiveReconnectState,
) {
    let load_revision = context.load_revision.load(Ordering::Acquire);
    if live_reconnect.accepted_load_revision != Some(load_revision)
        || context.published_load_revision.load(Ordering::Acquire) != load_revision
    {
        return;
    }
    let Ok(current) = context.state.lock().map(|state| state.clone()) else {
        return;
    };
    if !matches!(
        current.status,
        PlaybackStatus::Playing | PlaybackStatus::Reconnecting
    ) || !current.source.as_ref().is_some_and(is_live_source)
    {
        *live_reconnect = LiveReconnectState::default();
        return;
    }
    let was_reconnecting = current.status == PlaybackStatus::Reconnecting;
    live_reconnect.armed_source = current.source;
    live_reconnect.command_revision = context.command_revision.load(Ordering::Acquire);
    live_reconnect.retry_at = None;
    live_reconnect.readiness_deadline = None;
    arm_live_reconnect_stability(live_reconnect, Instant::now());
    if was_reconnecting {
        match update_shared_state(&context.state, |state| {
            state.status = PlaybackStatus::Playing;
            state.error = None;
        }) {
            Ok(next) => (context.listener)(next),
            Err(error) => report_native_playback_error(&format!(
                "Recovered Live Radio state could not be published: {}",
                error.message
            )),
        }
    }
}

fn start_live_reconnect(
    context: &PlaybackEventContext,
    live_reconnect: &mut LiveReconnectState,
) -> bool {
    let should_reconnect = context.state.lock().is_ok_and(|state| {
        matches!(
            state.status,
            PlaybackStatus::Playing | PlaybackStatus::Reconnecting
        ) && state.source == live_reconnect.armed_source
            && state.source.as_ref().is_some_and(is_live_source)
            && context.command_revision.load(Ordering::Acquire) == live_reconnect.command_revision
    });
    if !should_reconnect {
        return false;
    }
    if live_reconnect.retry_at.is_none() {
        schedule_next_live_reconnect(live_reconnect);
    }
    match update_shared_state(&context.state, |state| {
        state.status = PlaybackStatus::Reconnecting;
        state.error = None;
    }) {
        Ok(next) => (context.listener)(next),
        Err(error) => report_native_playback_error(&format!(
            "Live Radio reconnect state could not be published: {}",
            error.message
        )),
    }
    true
}

fn poll_live_reconnect(context: &PlaybackEventContext, live_reconnect: &mut LiveReconnectState) {
    if live_reconnect.armed_source.is_none() {
        return;
    }
    let Ok(current) = context.state.lock().map(|state| state.clone()) else {
        return;
    };
    if !is_current_live_reconnect(context, live_reconnect, &current) {
        *live_reconnect = LiveReconnectState::default();
        return;
    }
    let now = Instant::now();
    if update_live_reconnect_stability(live_reconnect, &current, now) {
        return;
    }
    schedule_expired_live_readiness(live_reconnect, now);
    if !is_live_reconnect_due(live_reconnect, now) {
        return;
    }
    run_live_reconnect_attempt(context, live_reconnect);
}

fn is_current_live_reconnect(
    context: &PlaybackEventContext,
    live_reconnect: &LiveReconnectState,
    current: &PlaybackSessionState,
) -> bool {
    current.source == live_reconnect.armed_source
        && context.command_revision.load(Ordering::Acquire) == live_reconnect.command_revision
        && matches!(
            current.status,
            PlaybackStatus::Playing | PlaybackStatus::Reconnecting
        )
}

fn update_live_reconnect_stability(
    live_reconnect: &mut LiveReconnectState,
    current: &PlaybackSessionState,
    now: Instant,
) -> bool {
    if current.status != PlaybackStatus::Playing {
        return false;
    }
    if live_reconnect
        .stability_deadline
        .is_some_and(|deadline| now >= deadline)
    {
        live_reconnect.attempt = 0;
        live_reconnect.stability_deadline = None;
    }
    true
}

fn arm_live_reconnect_stability(live_reconnect: &mut LiveReconnectState, now: Instant) {
    if live_reconnect.attempt > 0 && live_reconnect.stability_deadline.is_none() {
        live_reconnect.stability_deadline = Some(now + LIVE_RECONNECT_STABILITY_RESET);
    }
}

fn schedule_expired_live_readiness(live_reconnect: &mut LiveReconnectState, now: Instant) {
    if live_reconnect
        .readiness_deadline
        .is_some_and(|deadline| now >= deadline)
    {
        schedule_next_live_reconnect(live_reconnect);
    }
}

fn is_live_reconnect_due(live_reconnect: &LiveReconnectState, now: Instant) -> bool {
    live_reconnect
        .retry_at
        .is_some_and(|retry_at| now >= retry_at)
}

fn run_live_reconnect_attempt(
    context: &PlaybackEventContext,
    live_reconnect: &mut LiveReconnectState,
) {
    live_reconnect.retry_at = None;
    let result = reconnect_live_source(context, live_reconnect);
    if let Err(error) = result {
        report_native_playback_error(&format!("Live Radio reconnect attempt failed: {error}"));
        schedule_next_live_reconnect(live_reconnect);
        return;
    }
    live_reconnect.readiness_deadline = Some(Instant::now() + LIVE_RECONNECT_READINESS_TIMEOUT);
}

fn reconnect_live_source(
    context: &PlaybackEventContext,
    live_reconnect: &LiveReconnectState,
) -> Result<(), String> {
    if context.command_revision.load(Ordering::Acquire) != live_reconnect.command_revision {
        return Err("playback source changed before reconnect".to_owned());
    }
    let source = live_reconnect
        .armed_source
        .as_ref()
        .ok_or_else(|| "live playback source is unavailable".to_owned())?;
    let url = playback_url(source).map_err(|error| error.message)?;
    let process = context
        .process
        .lock()
        .map_err(|_| "Native mpv process is unavailable.".to_owned())?;
    if context.command_revision.load(Ordering::Acquire) != live_reconnect.command_revision {
        return Err("playback source changed while reconnect was waiting".to_owned());
    }
    let load_revision = context.load_revision.fetch_add(1, Ordering::AcqRel) + 1;
    process.load_with_event_boundary(&url, load_revision)?;
    process.set_paused(false)?;
    context
        .published_load_revision
        .store(load_revision, Ordering::Release);
    Ok(())
}

fn schedule_next_live_reconnect(live_reconnect: &mut LiveReconnectState) {
    live_reconnect.retry_at = Some(Instant::now() + live_reconnect_delay(live_reconnect.attempt));
    live_reconnect.readiness_deadline = None;
    live_reconnect.stability_deadline = None;
    live_reconnect.attempt += 1;
}

fn live_reconnect_delay(attempt: usize) -> Duration {
    let index = attempt.min(LIVE_RECONNECT_DELAY_STEPS.len() - 1);
    #[cfg(not(test))]
    {
        Duration::from_secs(LIVE_RECONNECT_DELAY_STEPS[index])
    }
    #[cfg(test)]
    {
        Duration::from_millis(LIVE_RECONNECT_DELAY_STEPS[index] * 10)
    }
}

fn is_reconnectable_live_interruption(event: &MpvEvent) -> bool {
    matches!(event, MpvEvent::Ended | MpvEvent::Error(_))
}

fn is_live_source(source: &Value) -> bool {
    matches!(
        source.get("type").and_then(Value::as_str),
        Some("radio-station" | "catalog-preview")
    )
}

fn apply_adaptive_decoder_rate(
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    rate_hz: Option<u32>,
) {
    let result = apply_adaptive_decoder_rate_inner(state, adaptive_system_rate, rate_hz);
    if let Err(error) = result {
        report_native_playback_error(&format!(
            "Adaptive System Rate decoder setup failed: {error}"
        ));
        cleanup_adaptive_decoder_failure(adaptive_system_rate);
        publish_adaptive_decoder_failure(state, listener);
    }
}

fn apply_adaptive_decoder_rate_inner(
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    rate_hz: Option<u32>,
) -> Result<(), String> {
    let is_adaptive = state
        .lock()
        .map_err(|_| "Native playback state is unavailable during decoder setup.".to_owned())?
        .output_mode
        == OutputMode::AdaptiveSystemRate;
    if !is_adaptive {
        return Ok(());
    }
    adaptive_system_rate
        .ok_or_else(|| "Adaptive System Rate is unavailable on this system.".to_owned())?
        .lock()
        .map_err(|_| "Adaptive System Rate state is unavailable.".to_owned())?
        .apply_source_sample_rate(rate_hz)
}

fn cleanup_adaptive_decoder_failure(
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) {
    let result = adaptive_system_rate
        .ok_or_else(|| "Adaptive System Rate controller is unavailable.".to_owned())
        .and_then(|adaptive| {
            adaptive
                .lock()
                .map_err(|_| "Adaptive System Rate state is unavailable.".to_owned())?
                .reset_for_recovery()
        });
    if let Err(error) = result {
        report_native_playback_error(&format!(
            "Adaptive System Rate cleanup failed after decoder setup: {error}"
        ));
    }
}

fn publish_adaptive_decoder_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
) {
    let result = update_shared_state(state, |state| {
        state.output_mode = OutputMode::System;
        set_error_state(
            state,
            "Adaptive System Rate could not follow this source. System Output was restored."
                .to_owned(),
        );
    });
    match result {
        Ok(next) => listener(next),
        Err(error) => report_native_playback_error(&format!(
            "Adaptive System Rate decoder failure could not be published: {}",
            error.message
        )),
    }
}

fn refresh_direct_output_device(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    output_devices: Option<&Mutex<OutputDeviceCatalog>>,
) {
    let Some(output_devices) = output_devices else {
        return;
    };
    match should_observe_direct_output(state) {
        Ok(false) => return,
        Err(error) => {
            report_native_playback_error(&error);
            return;
        }
        Ok(true) => {}
    }
    match observe_direct_output_devices(output_devices) {
        Ok((devices, Some(disconnected))) => {
            pause_disconnected_output(process, state, listener, devices, disconnected)
        }
        Ok((devices, None)) => publish_hotplug_devices(state, listener, devices),
        Err(error) => report_native_playback_error(&format!(
            "Output Device hotplug observation failed: {error}"
        )),
    }
}

fn should_observe_direct_output(state: &Arc<Mutex<PlaybackSessionState>>) -> Result<bool, String> {
    let state = state
        .lock()
        .map_err(|_| "Output Device hotplug playback state is unavailable.".to_owned())?;
    Ok(state.output_mode == OutputMode::DirectAlsa
        && !state
            .output_device_issue
            .as_ref()
            .is_some_and(|issue| issue.code == OutputDeviceIssueCode::Disconnected))
}

fn observe_direct_output_devices(
    output_devices: &Mutex<OutputDeviceCatalog>,
) -> Result<(Vec<OutputDevice>, Option<OutputDevice>), String> {
    let mut catalog = output_devices
        .lock()
        .map_err(|_| "Output Device hotplug catalog is unavailable.".to_owned())?;
    catalog
        .refresh()
        .map(|devices| (devices, None))
        .or_else(|error| match error {
            OutputDeviceError::SelectedDeviceDisconnected(device) => {
                Ok((catalog.devices().to_vec(), Some(device)))
            }
            error => Err(output_device_error_message(error)),
        })
}

fn publish_hotplug_devices(
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    devices: Vec<OutputDevice>,
) {
    let should_publish = match state.lock() {
        Ok(state) => state.available_output_devices != devices,
        Err(_) => {
            report_native_playback_error("Output Device hotplug playback state is unavailable.");
            return;
        }
    };
    if !should_publish {
        return;
    }
    match update_shared_state(state, |state| {
        state.available_output_devices = devices.clone()
    }) {
        Ok(next) => listener(next),
        Err(error) => report_native_playback_error(&format!(
            "Output Device hotplug state could not be published: {}",
            error.message
        )),
    }
}

fn pause_disconnected_output(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    devices: Vec<OutputDevice>,
    disconnected: OutputDevice,
) {
    if let Err(error) = process
        .lock()
        .map_err(|_| "Native mpv process is unavailable.".to_owned())
        .and_then(|process| process.set_paused(true))
    {
        report_native_playback_error(&format!(
            "Direct ALSA Output could not pause after disconnect: {error}"
        ));
    }
    match update_shared_state(state, |state| {
        state.available_output_devices = devices.clone();
        if state.source.is_some() {
            state.status = PlaybackStatus::Paused;
        }
        state.output_device_issue = Some(disconnected_device_issue(&disconnected));
    }) {
        Ok(next) => listener(next),
        Err(error) => eprintln!(
            "Output Device disconnect state could not be published: {}",
            error.message
        ),
    }
}

fn process_mpv_event(
    context: &PlaybackEventContext,
    event: MpvEvent,
    last_path_refresh: Instant,
) -> bool {
    let is_decoder_readiness = matches!(event, MpvEvent::Decoder(_));
    let is_stable_playback = is_stable_playback_event(&event);
    let should_refresh_path = matches!(event, MpvEvent::Time(_))
        && last_path_refresh.elapsed() >= PATH_REFRESH_INTERVAL
        && context.path_observer.is_some();
    match handle_mpv_event(
        &context.process,
        &context.state,
        &context.queue,
        event,
        &context.listener,
        context.path_observer.as_deref(),
        context.adaptive_system_rate.as_deref(),
    ) {
        Err(error) => eprintln!(
            "Native playback event state update failed: {}",
            error.message
        ),
        Ok(()) if is_stable_playback => {
            mark_recovered_player_stable(&context.state, &context.lifecycle, &context.listener)
        }
        Ok(()) => {}
    }
    if should_refresh_path && !is_decoder_readiness {
        refresh_pipewire_telemetry(
            &context.state,
            &context.listener,
            context.path_observer.as_deref(),
        );
    }
    is_decoder_readiness || should_refresh_path
}

fn is_stable_playback_event(event: &MpvEvent) -> bool {
    match event {
        MpvEvent::Decoder(_) => true,
        MpvEvent::Time(value) => value.is_finite() && *value >= 0.0,
        _ => false,
    }
}

fn recover_after_player_exit(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    starter: Option<&ProcessStarter>,
    listener: &StateListener,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    message: String,
) -> Option<Receiver<MpvEvent>> {
    report_native_playback_error(&message);
    match try_recover_after_player_exit(
        process,
        state,
        lifecycle,
        starter,
        listener,
        adaptive_system_rate,
    ) {
        Ok(events) => events,
        Err(failure) => {
            report_native_playback_error(&format!(
                "recovery failed [{}]: {}",
                failure.code, failure.diagnostic
            ));
            if let Err(error) =
                publish_lifecycle_failure(state, listener, failure.code, failure.message)
            {
                eprintln!(
                    "Native playback recovery error could not be published: {}",
                    error.message
                );
            }
            None
        }
    }
}

struct RecoveryFailure {
    code: &'static str,
    message: String,
    diagnostic: String,
}

impl RecoveryFailure {
    fn internal(code: &'static str, diagnostic: impl Into<String>) -> Self {
        Self {
            code,
            message: recovery_public_message(code).to_owned(),
            diagnostic: diagnostic.into(),
        }
    }

    fn actionable(code: &'static str, message: String) -> Self {
        Self {
            code,
            diagnostic: message.clone(),
            message,
        }
    }
}

fn recovery_public_message(code: &str) -> &'static str {
    match code {
        "mpv-recovery-state-unavailable" => {
            "Native playback could not recover because Playback Session state is unavailable. Restart the Desktop Client."
        }
        "mpv-recovery-snapshot-failed" => {
            "Native playback could not preserve the interrupted session. Quit and reopen the Desktop Client to try again."
        }
        "mpv-recovery-lifecycle-unavailable" => {
            "Native playback could not recover because lifecycle state is unavailable. Restart the Desktop Client."
        }
        "mpv-restart-unavailable" => {
            "Native playback cannot restart in this session. Restart the Desktop Client."
        }
        "mpv-recovery-transition-invalid" => {
            "Native playback could not complete recovery. Quit and reopen the Desktop Client to try again."
        }
        _ => "Native playback could not restart. Quit and reopen the Desktop Client to try again.",
    }
}

fn try_recover_after_player_exit(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    starter: Option<&ProcessStarter>,
    listener: &StateListener,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<Option<Receiver<MpvEvent>>, RecoveryFailure> {
    let (snapshot, should_resume) = interrupted_session_snapshot(state)?;
    reset_adaptive_for_recovery(adaptive_system_rate)?;
    let transition = lifecycle
        .lock()
        .map_err(|_| {
            RecoveryFailure::internal(
                "mpv-recovery-lifecycle-unavailable",
                "Native playback could not recover because lifecycle state is unavailable. Restart the Desktop Client.",
            )
        })?
        .unexpected_player_exit(&snapshot, should_resume);
    apply_recovery_transition(
        process,
        state,
        starter,
        listener,
        adaptive_system_rate,
        transition.action,
    )
}

fn reset_adaptive_for_recovery(
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<(), RecoveryFailure> {
    let Some(adaptive_system_rate) = adaptive_system_rate else {
        return Ok(());
    };
    adaptive_system_rate
        .lock()
        .map_err(|_| {
            RecoveryFailure::internal(
                "mpv-recovery-state-unavailable",
                "Adaptive System Rate state is unavailable during player recovery.",
            )
        })?
        .reset_for_recovery()
        .map_err(|error| RecoveryFailure::internal("mpv-restart-failed", error))
}

fn interrupted_session_snapshot(
    state: &Arc<Mutex<PlaybackSessionState>>,
) -> Result<(PlaybackSessionSnapshot, bool), RecoveryFailure> {
    let current = state.lock().map_err(|_| {
        RecoveryFailure::internal(
            "mpv-recovery-state-unavailable",
            "Native playback could not recover because its Playback Session state is unavailable. Restart the Desktop Client.",
        )
    })?.clone();
    let snapshot = PlaybackSessionSnapshot::from_serializable_state(&current).map_err(|error| {
        RecoveryFailure::internal(
            "mpv-recovery-snapshot-failed",
            format!("Native playback could not snapshot the interrupted session: {error}"),
        )
    })?;
    Ok((snapshot, current.status.is_active()))
}

fn apply_recovery_transition(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    starter: Option<&ProcessStarter>,
    listener: &StateListener,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    action: PlaybackLifecycleAction,
) -> Result<Option<Receiver<MpvEvent>>, RecoveryFailure> {
    match action {
        PlaybackLifecycleAction::RestartPlayer {
            snapshot,
            should_resume,
        } => {
            let starter = starter.ok_or_else(|| {
                RecoveryFailure::internal(
                    "mpv-restart-unavailable",
                    "Native playback cannot restart mpv in this session. Restart the Desktop Client.",
                )
            })?;
            restart_player(
                process,
                state,
                starter,
                listener,
                adaptive_system_rate,
                &snapshot,
                should_resume,
            )
            .map(Some)
        }
        PlaybackLifecycleAction::SurfaceActionableError(error) => {
            Err(RecoveryFailure::actionable(error.code, error.message))
        }
        invalid_action => Err(RecoveryFailure::internal(
            "mpv-recovery-transition-invalid",
            format!(
                "Native playback recovery produced an invalid lifecycle action: {invalid_action:?}"
            ),
        )),
    }
}

fn restart_player(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    starter: &ProcessStarter,
    listener: &StateListener,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    snapshot: &PlaybackSessionSnapshot,
    should_resume: bool,
) -> Result<Receiver<MpvEvent>, RecoveryFailure> {
    let (next_process, next_events) =
        starter().map_err(|message| RecoveryFailure::internal("mpv-restart-failed", message))?;
    if let Some(adaptive_system_rate) = adaptive_system_rate {
        let rate_hz = snapshot
            .source()
            .map(SourceObservation::from_playback_source)
            .and_then(|source| source.format.sample_rate_hz);
        adaptive_system_rate
            .lock()
            .map_err(|_| {
                RecoveryFailure::internal(
                    "mpv-recovery-state-unavailable",
                    "Adaptive System Rate state is unavailable during player recovery.",
                )
            })?
            .apply_source_sample_rate(rate_hz)
            .map_err(|error| RecoveryFailure::internal("mpv-restart-failed", error))?;
    }
    let result = finish_player_restart(
        process,
        state,
        listener,
        snapshot,
        should_resume,
        next_process,
        next_events,
    );
    reset_reapplied_rate_after_failed_restart(result, adaptive_system_rate)
}

fn finish_player_restart(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    snapshot: &PlaybackSessionSnapshot,
    should_resume: bool,
    next_process: Box<dyn MpvProcessAdapter>,
    next_events: Receiver<MpvEvent>,
) -> Result<Receiver<MpvEvent>, RecoveryFailure> {
    let playback_rate = state
        .lock()
        .map_err(|_| {
            RecoveryFailure::internal(
                "mpv-recovery-state-unavailable",
                "Playback speed is unavailable during player recovery.",
            )
        })?
        .playback_rate;
    apply_snapshot_to_process(
        next_process.as_ref(),
        snapshot,
        should_resume,
        playback_rate,
    )
    .map_err(|message| RecoveryFailure::internal("mpv-restart-failed", message))?;
    *process.lock().map_err(|_| {
        RecoveryFailure::internal(
            "mpv-restart-failed",
            "Native mpv process state is unavailable.",
        )
    })? = next_process;
    let next = update_shared_state(state, |state| {
        restore_snapshot_state(state, snapshot);
        if should_resume && state.source.is_some() {
            state.status = PlaybackStatus::Playing;
        }
    })
    .map_err(|error| RecoveryFailure::internal("mpv-recovery-state-unavailable", error.message))?;
    listener(next);
    Ok(next_events)
}

fn reset_reapplied_rate_after_failed_restart<T>(
    result: Result<T, RecoveryFailure>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<T, RecoveryFailure> {
    let failure = match result {
        Ok(value) => return Ok(value),
        Err(failure) => failure,
    };
    if let Err(cleanup_failure) = reset_adaptive_for_recovery(adaptive_system_rate) {
        return Err(RecoveryFailure::internal(
            failure.code,
            format!(
                "{} Adaptive System Rate cleanup also failed: {}",
                failure.diagnostic, cleanup_failure.diagnostic
            ),
        ));
    }
    Err(failure)
}

fn publish_lifecycle_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    code: &str,
    message: String,
) -> Result<(), PlaybackCommandError> {
    let next = update_shared_state(state, |state| {
        state.status = PlaybackStatus::Error;
        state.error = Some(PlaybackStateError {
            code: code.to_owned(),
            message: message.clone(),
        });
    })?;
    listener(next);
    Ok(())
}

fn mark_recovered_player_stable(
    state: &Arc<Mutex<PlaybackSessionState>>,
    lifecycle: &Arc<Mutex<PlaybackLifecycle>>,
    listener: &StateListener,
) {
    if let Ok(mut lifecycle) = lifecycle.lock() {
        lifecycle.player_stabilized();
        return;
    }
    let message = "Native playback became stable, but recovery lifecycle state is unavailable. Restart the Desktop Client.".to_owned();
    report_native_playback_error(&message);
    if let Err(error) = publish_lifecycle_failure(
        state,
        listener,
        "mpv-recovery-lifecycle-unavailable",
        message,
    ) {
        eprintln!(
            "Native playback lifecycle failure could not be published: {}",
            error.message
        );
    }
}

fn handle_mpv_event(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    queue: &Arc<Mutex<PlaybackQueueContext>>,
    event: MpvEvent,
    listener: &StateListener,
    path_observer: Option<&dyn PipeWireObserver>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<(), PlaybackCommandError> {
    let next = reduce_mpv_event(
        process,
        state,
        queue,
        event,
        path_observer,
        adaptive_system_rate,
    )?;
    if let Some(next) = next {
        listener(next);
    }
    Ok(())
}

fn reduce_mpv_event(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    queue: &Arc<Mutex<PlaybackQueueContext>>,
    event: MpvEvent,
    path_observer: Option<&dyn PipeWireObserver>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<Option<PlaybackSessionState>, PlaybackCommandError> {
    let result = match event {
        MpvEvent::Time(value) if value.is_finite() && value >= 0.0 => {
            update_shared_state(state, |state| state.current_time = value)
        }
        MpvEvent::Duration(value) if value.is_finite() && value > 0.0 => {
            update_shared_state(state, |state| state.duration = value)
        }
        MpvEvent::BufferedEnd(value) if value.is_finite() && value >= 0.0 => {
            update_shared_state(state, |state| state.buffered_end = Some(value))
        }
        MpvEvent::Paused(is_paused) => update_paused_state(state, is_paused),
        MpvEvent::Decoder(decoder) => {
            update_decoder_telemetry(process, state, decoder, path_observer)
        }
        MpvEvent::Ended => handle_ended_event(process, state, queue, adaptive_system_rate),
        MpvEvent::Error(message) => handle_mpv_error(process, state, adaptive_system_rate, message),
        MpvEvent::LoadBoundary(_)
        | MpvEvent::ExitedUnexpectedly(_)
        | MpvEvent::Time(_)
        | MpvEvent::Duration(_)
        | MpvEvent::BufferedEnd(_) => {
            return Ok(None);
        }
    };
    result.map(Some)
}

fn handle_mpv_error(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    message: String,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    report_native_playback_error(&message);
    let cleanup =
        reset_adaptive_after_event_failure(state, adaptive_system_rate, "mpv error event");
    if cleanup.is_err() {
        return publish_adaptive_cleanup_failure(state);
    }
    match direct_error_device(state)? {
        Some(device) => publish_direct_mpv_error(process, state, device),
        None => update_shared_state(state, |state| {
            set_error_state(state, PLAYBACK_FAILED_MESSAGE.to_owned())
        }),
    }
}

fn direct_error_device(
    state: &Arc<Mutex<PlaybackSessionState>>,
) -> Result<Option<OutputDevice>, PlaybackCommandError> {
    let state = state
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?;
    Ok((state.output_mode == OutputMode::DirectAlsa)
        .then(|| state.selected_output_device.clone())
        .flatten())
}

fn publish_direct_mpv_error(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    device: OutputDevice,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let process = process
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native mpv process is unavailable."))?;
    process
        .set_paused(true)
        .map_err(PlaybackCommandError::new)?;
    process
        .configure_output(&MpvOutputConfiguration::System)
        .map_err(PlaybackCommandError::new)?;
    update_shared_state(state, |state| {
        state.status = PlaybackStatus::Paused;
        state.output_mode = OutputMode::System;
        state.output_device_issue = Some(busy_or_unsupported_device_issue(&device));
        reset_output_telemetry(state);
    })
}

fn update_paused_state(
    state: &Arc<Mutex<PlaybackSessionState>>,
    is_paused: bool,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_shared_state(state, |state| {
        if state.source.is_some() && state.status != PlaybackStatus::Ended {
            state.status = if is_paused {
                PlaybackStatus::Paused
            } else {
                PlaybackStatus::Playing
            };
        }
    })
}

fn update_decoder_telemetry(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    decoder: ObservedMpvProperties,
    path_observer: Option<&dyn PipeWireObserver>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let pipewire = observe_pipewire_path(path_observer);
    let negotiated_format = observe_ready_direct_format(process, state)?;
    update_shared_state(state, |state| {
        if state.output_mode == OutputMode::DirectAlsa
            && let Some(device) = state.selected_output_device.as_ref()
        {
            state.telemetry = PlaybackTelemetry::native_direct_alsa_output(
                source_observation(state.source.as_ref()),
                decoder.clone(),
                &device.name,
                negotiated_format.clone(),
                processing_observation(&state.processing),
            );
            return;
        }
        state.telemetry = telemetry_for_output_mode(
            state,
            source_observation(state.source.as_ref()),
            decoder.clone(),
            pipewire.clone(),
        );
    })
}

fn observe_ready_direct_format(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
) -> Result<Option<AudioFormatObservation>, PlaybackCommandError> {
    let is_direct = state
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?
        .output_mode
        == OutputMode::DirectAlsa;
    if !is_direct {
        return Ok(None);
    }
    let result = process
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native mpv process is unavailable."))?
        .observe_output_format();
    match result {
        Ok(format) => Ok(format),
        Err(error) => {
            report_native_playback_error(&format!(
                "Direct ALSA ready output format observation failed: {error}"
            ));
            Ok(None)
        }
    }
}

fn source_observation(source: Option<&Value>) -> SourceObservation {
    source
        .map(SourceObservation::from_playback_source)
        .unwrap_or_else(SourceObservation::unknown)
}

fn telemetry_for_output_mode(
    state: &PlaybackSessionState,
    source: SourceObservation,
    decoder: ObservedMpvProperties,
    pipewire: Option<PipeWireObservation>,
) -> PlaybackTelemetry {
    if state.output_mode == OutputMode::DirectAlsa
        && let Some(device) = state.selected_output_device.as_ref()
    {
        return PlaybackTelemetry::native_direct_alsa_output(
            source,
            decoder,
            &device.name,
            (state.telemetry.device.format != AudioFormatObservation::unknown())
                .then(|| state.telemetry.device.format.clone()),
            processing_observation(&state.processing),
        );
    }
    PlaybackTelemetry::native_system_output(
        source,
        decoder,
        pipewire,
        processing_observation(&state.processing),
    )
}

fn reset_output_telemetry(state: &mut PlaybackSessionState) {
    state.telemetry = telemetry_for_output_mode(
        state,
        source_observation(state.source.as_ref()),
        state.telemetry.decoder.clone(),
        None,
    );
}

fn publish_system_output(state: &mut PlaybackSessionState) {
    state.output_mode = OutputMode::System;
    state.output_device_issue = None;
    state.telemetry = PlaybackTelemetry::native_system_output(
        source_observation(state.source.as_ref()),
        state.telemetry.decoder.clone(),
        None,
        processing_observation(&state.processing),
    );
}

fn publish_adaptive_system_output(state: &mut PlaybackSessionState) {
    state.output_mode = OutputMode::AdaptiveSystemRate;
    state.output_device_issue = None;
    state.telemetry = PlaybackTelemetry::native_system_output(
        source_observation(state.source.as_ref()),
        state.telemetry.decoder.clone(),
        None,
        processing_observation(&state.processing),
    );
}

fn output_device_public_message(error: &OutputDeviceError) -> &'static str {
    match error {
        OutputDeviceError::EnumerationFailed(_) => {
            "Raw ALSA Output Devices could not be listed. Check the local ALSA setup and try again."
        }
        OutputDeviceError::NotRawHardware => {
            "Choose a raw ALSA hardware device from the available Output Device list."
        }
        OutputDeviceError::SelectedDeviceDisconnected(_) | OutputDeviceError::Unavailable => {
            "The selected raw ALSA Output Device is unavailable. Refresh the device list and choose another device."
        }
    }
}

fn output_device_error_message(error: OutputDeviceError) -> String {
    report_native_playback_error(&format!("Output Device operation failed: {error:?}"));
    output_device_public_message(&error).to_owned()
}

fn busy_or_unsupported_device_issue(device: &OutputDevice) -> OutputDeviceIssue {
    OutputDeviceIssue {
        code: OutputDeviceIssueCode::BusyOrUnsupported,
        message: format!(
            "{} is busy or does not support this source format. Select Normal Output to continue through the system output.",
            device.name
        ),
    }
}

fn disconnected_device_issue(device: &OutputDevice) -> OutputDeviceIssue {
    OutputDeviceIssue {
        code: OutputDeviceIssueCode::Disconnected,
        message: format!(
            "{} disconnected. Select Normal Output to continue through the active system output.",
            device.name
        ),
    }
}

fn observe_pipewire_path(observer: Option<&dyn PipeWireObserver>) -> Option<PipeWireObservation> {
    match observer?.observe() {
        Ok(observation) => observation,
        Err(error) => {
            eprintln!("PipeWire playback observation failed: {error}");
            None
        }
    }
}

fn refresh_pipewire_telemetry(
    state: &Arc<Mutex<PlaybackSessionState>>,
    listener: &StateListener,
    observer: Option<&dyn PipeWireObserver>,
) {
    let pipewire = observe_pipewire_path(observer);
    match update_shared_state(state, |state| {
        state.telemetry = telemetry_for_output_mode(
            state,
            source_observation(state.source.as_ref()),
            state.telemetry.decoder.clone(),
            pipewire.clone(),
        );
    }) {
        Ok(next) => listener(next),
        Err(error) => eprintln!(
            "PipeWire telemetry refresh could not update playback state: {}",
            error.message
        ),
    }
}

fn handle_ended_event(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    queue: &Arc<Mutex<PlaybackQueueContext>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let (repeat_mode, stop_after_current) = {
        let state = state
            .lock()
            .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?;
        (state.repeat_mode.clone(), state.stop_after_current)
    };
    if stop_after_current {
        return update_shared_state(state, |state| {
            state.stop_after_current = false;
            state.status = PlaybackStatus::Ended;
        });
    }
    if repeat_mode == RepeatMode::Once {
        return repeat_current_source(process, state, adaptive_system_rate);
    }
    let next_source = queue
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?
        .adjacent_source(QueueDirection::Next, repeat_mode == RepeatMode::Loop);
    if let Some(source) = next_source {
        return load_advanced_source(process, state, queue, adaptive_system_rate, source);
    }
    update_shared_state(state, |state| {
        state.status = PlaybackStatus::Ended;
    })
}

fn repeat_current_source(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let result = process
        .lock()
        .map_err(|_| "Native mpv process is unavailable.".to_owned())
        .and_then(|process| {
            process.seek(0.0)?;
            process.set_paused(false)
        });
    if let Err(message) = result {
        if reset_adaptive_after_event_failure(state, adaptive_system_rate, "Repeat Once restart")
            .is_err()
        {
            report_native_playback_error(&message);
            return publish_adaptive_cleanup_failure(state);
        }
        return publish_native_command_failure(state, message);
    }
    update_shared_state(state, |state| {
        state.current_time = 0.0;
        state.buffered_end = None;
        state.status = PlaybackStatus::Playing;
        state.repeat_mode = RepeatMode::Off;
    })
}

fn publish_adaptive_cleanup_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_shared_state(state, |state| {
        set_error_state(state, ADAPTIVE_CLEANUP_FAILED_MESSAGE.to_owned())
    })
}

fn load_advanced_source(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    state: &Arc<Mutex<PlaybackSessionState>>,
    queue: &Arc<Mutex<PlaybackQueueContext>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    source: Value,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    prepare_advanced_source_rate(state, adaptive_system_rate, &source)?;
    if let Err(message) = start_advanced_source(process, &source) {
        return publish_advanced_source_failure(state, adaptive_system_rate, message);
    }
    align_and_publish_advanced_source(state, queue, source)
}

fn prepare_advanced_source_rate(
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    source: &Value,
) -> Result<(), PlaybackCommandError> {
    if let Err(error) = apply_adaptive_rate_for_event(state, adaptive_system_rate, source) {
        if reset_adaptive_after_event_failure(
            state,
            adaptive_system_rate,
            "automatic queue rate setup",
        )
        .is_err()
        {
            return Err(PlaybackCommandError::new(ADAPTIVE_CLEANUP_FAILED_MESSAGE));
        }
        return Err(error);
    }
    Ok(())
}

fn start_advanced_source(
    process: &Arc<Mutex<Box<dyn MpvProcessAdapter>>>,
    source: &Value,
) -> Result<(), String> {
    let url = playback_url(source).map_err(|error| error.message)?;
    process
        .lock()
        .map_err(|_| "Native mpv process is unavailable.".to_owned())
        .and_then(|process| {
            process.load(&url)?;
            process.set_paused(false)
        })
}

fn publish_advanced_source_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    message: String,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    if reset_adaptive_after_event_failure(state, adaptive_system_rate, "automatic queue advance")
        .is_err()
    {
        report_native_playback_error(&message);
        return publish_adaptive_cleanup_failure(state);
    }
    publish_native_command_failure(state, message)
}

fn align_and_publish_advanced_source(
    state: &Arc<Mutex<PlaybackSessionState>>,
    queue: &Arc<Mutex<PlaybackQueueContext>>,
    source: Value,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    queue
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback Queue context is unavailable."))?
        .align_to_source(&source)
        .map_err(PlaybackCommandError::new)?;
    update_shared_state(state, |state| {
        set_playing_source(state, source.clone(), None)
    })
}

fn apply_adaptive_rate_for_event(
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    source: &Value,
) -> Result<(), PlaybackCommandError> {
    let is_adaptive = state
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?
        .output_mode
        == OutputMode::AdaptiveSystemRate;
    if !is_adaptive {
        return Ok(());
    }
    let rate_hz = SourceObservation::from_playback_source(source)
        .format
        .sample_rate_hz;
    adaptive_system_rate
        .ok_or_else(|| PlaybackCommandError::new("Adaptive System Rate is unavailable."))?
        .lock()
        .map_err(|_| PlaybackCommandError::new("Adaptive System Rate state is unavailable."))?
        .apply_source_sample_rate(rate_hz)
        .map_err(PlaybackCommandError::new)
}

fn reset_adaptive_after_event_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
    adaptive_system_rate: Option<&Mutex<AdaptiveSystemRateController>>,
    context: &str,
) -> Result<(), PlaybackCommandError> {
    let is_adaptive = state
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?
        .output_mode
        == OutputMode::AdaptiveSystemRate;
    if !is_adaptive {
        return Ok(());
    }
    let result = adaptive_system_rate
        .ok_or_else(|| PlaybackCommandError::new("Adaptive System Rate is unavailable."))?
        .lock()
        .map_err(|_| PlaybackCommandError::new("Adaptive System Rate state is unavailable."))?
        .reset_for_recovery();
    result.map_err(|error| {
        report_native_playback_error(&format!(
            "Adaptive System Rate cleanup failed after {context}: {error}"
        ));
        PlaybackCommandError::new("Adaptive System Rate cleanup failed.")
    })
}

fn set_playing_source(
    state: &mut PlaybackSessionState,
    source: Value,
    negotiated_format: Option<AudioFormatObservation>,
) {
    state.duration = track_duration(&source);
    state.processing.effective_replay_gain_mode =
        effective_replay_gain_mode(state.processing.replay_gain_mode, Some(&source));
    state.telemetry = telemetry_for_output_mode(
        state,
        SourceObservation::from_playback_source(&source),
        ObservedMpvProperties::unknown(),
        None,
    );
    if state.output_mode == OutputMode::DirectAlsa {
        state.telemetry.device.format =
            negotiated_format.unwrap_or_else(AudioFormatObservation::unknown);
    }
    state.source = Some(source);
    state.status = PlaybackStatus::Playing;
    state.current_time = 0.0;
    state.buffered_end = None;
    state.error = None;
}

fn update_playing_state(state: &mut PlaybackSessionState, source: Value, is_new_source: bool) {
    if is_new_source {
        set_playing_source(state, source, None);
        return;
    }
    state.source = Some(source);
    state.status = PlaybackStatus::Playing;
    state.error = None;
}

fn publish_native_command_failure(
    state: &Arc<Mutex<PlaybackSessionState>>,
    message: String,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    report_native_playback_error(&message);
    update_shared_state(state, |state| {
        set_error_state(state, PLAYBACK_FAILED_MESSAGE.to_owned())
    })
}

fn update_shared_state(
    state: &Arc<Mutex<PlaybackSessionState>>,
    change: impl FnOnce(&mut PlaybackSessionState),
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut state = state
        .lock()
        .map_err(|_| PlaybackCommandError::new("Native playback state is unavailable."))?;
    change(&mut state);
    Ok(state.clone())
}

fn set_error_state(state: &mut PlaybackSessionState, message: String) {
    state.status = PlaybackStatus::Error;
    state.error = Some(PlaybackStateError {
        code: "playback-failed".to_owned(),
        message,
    });
}

fn report_native_playback_error(detail: &str) {
    eprintln!("Native playback process failed: {detail}");
}

fn playback_url(source: &Value) -> Result<String, PlaybackCommandError> {
    let parsed = PlaybackSource::parse(source).map_err(PlaybackCommandError::new)?;
    Ok(parsed
        .playback_url()
        .map_err(PlaybackCommandError::new)?
        .to_owned())
}

fn track_duration(source: &Value) -> f64 {
    match PlaybackSource::parse(source) {
        Ok(source) => source.duration_seconds(),
        Err(error) => {
            eprintln!("Playback Source duration could not be inspected: {error}");
            0.0
        }
    }
}

fn processing_observation(state: &ProcessingState) -> ProcessingObservation {
    ProcessingObservation {
        profile: match state.profile {
            crate::processing::ProcessingProfile::Direct => "direct",
            crate::processing::ProcessingProfile::Processed => "processed",
        }
        .to_owned(),
        software_volume: Some(state.software_volume),
        replay_gain_mode: match state.replay_gain_mode {
            ReplayGainMode::Off => "off",
            ReplayGainMode::Track => "track",
            ReplayGainMode::Album => "album",
        }
        .to_owned(),
        effective_replay_gain_mode: match state.effective_replay_gain_mode {
            EffectiveReplayGainMode::Off => "off",
            EffectiveReplayGainMode::Track => "track",
            EffectiveReplayGainMode::Album => "album",
            EffectiveReplayGainMode::TrackFallback => "track-fallback",
            EffectiveReplayGainMode::Unavailable => "unavailable",
            EffectiveReplayGainMode::Unknown => "unknown",
        }
        .to_owned(),
        is_equalizer_enabled: Some(state.equalizer.is_enabled),
    }
}

fn effective_replay_gain_mode(
    mode: ReplayGainMode,
    source: Option<&Value>,
) -> EffectiveReplayGainMode {
    let track_gain = source
        .and_then(|value| value.pointer("/track/replayGain/trackGainDb"))
        .is_some_and(Value::is_number);
    let album_gain = source
        .and_then(|value| value.pointer("/track/replayGain/albumGainDb"))
        .is_some_and(Value::is_number);
    match mode {
        ReplayGainMode::Off => EffectiveReplayGainMode::Off,
        ReplayGainMode::Track if track_gain => EffectiveReplayGainMode::Track,
        ReplayGainMode::Album if album_gain => EffectiveReplayGainMode::Album,
        ReplayGainMode::Album if track_gain => EffectiveReplayGainMode::TrackFallback,
        ReplayGainMode::Track | ReplayGainMode::Album => EffectiveReplayGainMode::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LiveReconnectState, MpvEvent, MpvOutputConfiguration, MpvProcessAdapter, MpvWorkerRequest,
        OutputDeviceIssueCode, PATH_REFRESH_INTERVAL, PlaybackController, PlaybackSessionState,
        PlaybackStatus, RealMpvProcess, arm_live_reconnect_stability,
        update_live_reconnect_stability,
    };
    use crate::adaptive_system_rate::{
        AdaptiveCleanupMarker, AdaptiveSystemRateController, PipeWireRateAdapter,
    };
    use crate::exclusive_output::ExclusiveOutputCoordinator;
    use crate::output_device::{AlsaOutputDeviceAdapter, OutputDevice};
    use crate::playback_app_actions::{
        DesktopPlaybackAction, DesktopPlaybackShell, dispatch_desktop_playback_action,
    };
    use crate::playback_lifecycle::{
        PlaybackLifecycle, PlaybackSessionSnapshot, PlaybackSnapshotStore,
    };
    use crate::playback_test_support::{ControlledStreamServer, generate_audio_fixture};
    use crate::processing::{
        AppliedMpvProcessingConfiguration, EffectiveReplayGainMode, MpvProcessingConfiguration,
        OutputMode, ProcessingState, ReplayGainMode,
    };
    use crate::telemetry::{
        AudioFormatObservation, ObservedMpvProperties, PipeWireObservation, PipeWireObserver,
        SourceObservation,
    };
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    struct FakeMpvProcess {
        loaded_url: Arc<Mutex<Option<String>>>,
        is_shutdown: Arc<AtomicBool>,
        load_error: Option<String>,
    }

    struct ReconnectMpvProcess {
        load_calls: Arc<AtomicUsize>,
    }

    struct RecordedExclusiveOutput {
        actions: Arc<Mutex<Vec<String>>>,
        is_active: AtomicBool,
    }

    impl ExclusiveOutputCoordinator for RecordedExclusiveOutput {
        fn acquire(&self, device_id: &str) -> Result<(), String> {
            self.actions
                .lock()
                .expect("output actions")
                .push(format!("reserve:{device_id}"));
            self.is_active.store(true, Ordering::Release);
            Ok(())
        }

        fn release(&self) -> Result<(), String> {
            if self.is_active.swap(false, Ordering::AcqRel) {
                self.actions
                    .lock()
                    .expect("output actions")
                    .push("release".to_owned());
            }
            Ok(())
        }
    }

    fn record_real_mpv_output_actions(configurations: &[MpvOutputConfiguration]) -> Vec<String> {
        let (request_sender, request_receiver) = std::sync::mpsc::channel();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let recorded_actions = actions.clone();
        let recorder = thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                if let MpvWorkerRequest::Command {
                    command, response, ..
                } = request
                {
                    let command_name = command
                        .as_array()
                        .and_then(|values| values.first())
                        .and_then(serde_json::Value::as_str);
                    if command_name == Some("set_property") {
                        recorded_actions
                            .lock()
                            .expect("output actions")
                            .push(format!("mpv:{command}"));
                    }
                    response.send(Ok(json!(null))).expect("record mpv response");
                }
            }
        });
        let process = RealMpvProcess {
            requests: request_sender,
            worker: Mutex::new(None),
            ipc_directory: temporary_path("record-output-commands"),
            exclusive_output: Arc::new(RecordedExclusiveOutput {
                actions: actions.clone(),
                is_active: AtomicBool::new(false),
            }),
        };

        for configuration in configurations {
            process
                .configure_output(configuration)
                .expect("configure recorded mpv output");
        }
        drop(process);
        recorder.join().expect("join mpv command recorder");
        Arc::try_unwrap(actions)
            .expect("release output actions")
            .into_inner()
            .expect("output actions")
    }

    #[derive(Clone)]
    struct FakeOutputDevices {
        devices: Arc<Mutex<Vec<OutputDevice>>>,
    }

    #[derive(Default)]
    struct RecordedPipeWireRate {
        forced_rate_hz: Mutex<Option<u32>>,
        has_restored_automatic: AtomicBool,
    }

    impl PipeWireRateAdapter for RecordedPipeWireRate {
        fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
            Ok(*self.forced_rate_hz.lock().expect("PipeWire force-rate"))
        }

        fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
            *self.forced_rate_hz.lock().expect("PipeWire force-rate") = rate_hz;
            if rate_hz.is_none() {
                self.has_restored_automatic.store(true, Ordering::Release);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingResetPipeWireRate {
        forced_rate_hz: Mutex<Option<u32>>,
    }

    struct FailingCleanupMarker;

    impl AdaptiveCleanupMarker for FailingCleanupMarker {
        fn is_required(&self) -> Result<bool, String> {
            Ok(false)
        }

        fn mark_required(&self) -> Result<(), String> {
            Err("write /home/private-user/.config/secret-marker: permission denied".to_owned())
        }

        fn clear(&self) -> Result<(), String> {
            Ok(())
        }
    }

    impl PipeWireRateAdapter for FailingResetPipeWireRate {
        fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
            Ok(*self.forced_rate_hz.lock().expect("PipeWire force-rate"))
        }

        fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
            let mut forced_rate_hz = self.forced_rate_hz.lock().expect("PipeWire force-rate");
            if rate_hz.is_none() && forced_rate_hz.is_some() {
                return Err("controlled PipeWire reset failure".to_owned());
            }
            *forced_rate_hz = rate_hz;
            Ok(())
        }
    }

    impl FakeOutputDevices {
        fn replace(&self, devices: Vec<OutputDevice>) {
            *self.devices.lock().expect("fake output devices") = devices;
        }
    }

    impl AlsaOutputDeviceAdapter for FakeOutputDevices {
        fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String> {
            Ok(self.devices.lock().expect("fake output devices").clone())
        }
    }

    struct FailingOutputDevices;

    impl AlsaOutputDeviceAdapter for FailingOutputDevices {
        fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String> {
            Err("aplay /home/private-user exited: secret-device-token".to_owned())
        }
    }

    struct OutputConfiguredMpv {
        configured: Arc<Mutex<Vec<MpvOutputConfiguration>>>,
        is_paused: Arc<AtomicBool>,
        direct_configuration_error: Option<String>,
        negotiated_format: Option<AudioFormatObservation>,
    }

    struct ReobservedOutputMpv {
        configured_format: Option<AudioFormatObservation>,
        observed_format: Arc<Mutex<Option<AudioFormatObservation>>>,
        observation_calls: Arc<AtomicUsize>,
    }

    struct FailingAdvanceMpv {
        load_calls: AtomicUsize,
    }

    struct FailingRepeatMpv;

    impl MpvProcessAdapter for FailingRepeatMpv {
        fn load(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Err("controlled Repeat Once seek failure".to_owned())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl MpvProcessAdapter for FailingAdvanceMpv {
        fn load(&self, _url: &str) -> Result<(), String> {
            if self.load_calls.fetch_add(1, Ordering::Relaxed) > 0 {
                return Err("controlled queue advance load failure".to_owned());
            }
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl MpvProcessAdapter for ReobservedOutputMpv {
        fn load(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn configure_output(
            &self,
            _configuration: &MpvOutputConfiguration,
        ) -> Result<Option<AudioFormatObservation>, String> {
            Ok(self.configured_format.clone())
        }

        fn observe_output_format(&self) -> Result<Option<AudioFormatObservation>, String> {
            self.observation_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .observed_format
                .lock()
                .expect("observed output format")
                .clone())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl MpvProcessAdapter for OutputConfiguredMpv {
        fn load(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }

        fn set_paused(&self, is_paused: bool) -> Result<(), String> {
            self.is_paused.store(is_paused, Ordering::Release);
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn configure_output(
            &self,
            configuration: &MpvOutputConfiguration,
        ) -> Result<Option<AudioFormatObservation>, String> {
            self.configured
                .lock()
                .expect("output configurations")
                .push(configuration.clone());
            if matches!(configuration, MpvOutputConfiguration::DirectAlsa { .. })
                && let Some(error) = self.direct_configuration_error.as_ref()
            {
                return Err(error.clone());
            }
            Ok(self.negotiated_format.clone())
        }

        fn observe_output_format(&self) -> Result<Option<AudioFormatObservation>, String> {
            Ok(self.negotiated_format.clone())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    #[derive(Default)]
    struct TestDesktopPlaybackShell {
        is_hidden: AtomicBool,
        has_exited: AtomicBool,
    }

    impl DesktopPlaybackShell for TestDesktopPlaybackShell {
        fn show_main_window(&self) -> Result<(), String> {
            Ok(())
        }

        fn hide_main_window(&self) -> Result<(), String> {
            self.is_hidden.store(true, Ordering::Release);
            Ok(())
        }

        fn exit(&self) {
            self.has_exited.store(true, Ordering::Release);
        }
    }

    struct ObservedPipeWirePath {
        calls: AtomicUsize,
    }

    struct ObservedProcessingMpv {
        requested: Arc<Mutex<Option<MpvProcessingConfiguration>>>,
    }

    impl MpvProcessAdapter for ObservedProcessingMpv {
        fn load(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn apply_processing(
            &self,
            configuration: &MpvProcessingConfiguration,
        ) -> Result<AppliedMpvProcessingConfiguration, String> {
            *self.requested.lock().expect("requested processing") = Some(configuration.clone());
            Ok(AppliedMpvProcessingConfiguration {
                volume_percent: 37.0,
                replay_gain_mode: ReplayGainMode::Track,
                audio_filters: vec!["lavfi=[equalizer=f=1000:g=1.5]".to_owned()],
            })
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl PipeWireObserver for ObservedPipeWirePath {
        fn observe(&self) -> Result<Option<PipeWireObservation>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(Some(PipeWireObservation {
                graph_format: AudioFormatObservation {
                    sample_rate_hz: Some(96_000),
                    bit_depth: Some(24),
                    channels: Some(2),
                },
                is_graph_resampling: Some(false),
                device_name: Some("Routed USB DAC".to_owned()),
                device_format: AudioFormatObservation {
                    sample_rate_hz: Some(96_000),
                    bit_depth: Some(24),
                    channels: Some(2),
                },
                is_device_resampling: Some(false),
            }))
        }
    }

    #[derive(Default)]
    struct RestoredProcessState {
        loaded_url: Option<String>,
        is_paused: bool,
        playhead_seconds: f64,
        standalone_seek_calls: usize,
        volume: f64,
        playback_rate: f64,
    }

    struct RestorableMpvProcess {
        state: Arc<Mutex<RestoredProcessState>>,
    }

    struct AsynchronousLoadMpvProcess;

    impl MpvProcessAdapter for AsynchronousLoadMpvProcess {
        fn load(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }

        fn load_at(&self, _url: &str, _start_seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Err("file has not finished loading".to_owned())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl MpvProcessAdapter for RestorableMpvProcess {
        fn load(&self, url: &str) -> Result<(), String> {
            self.state.lock().expect("process state").loaded_url = Some(url.to_owned());
            Ok(())
        }

        fn load_at(&self, url: &str, seconds: f64) -> Result<(), String> {
            let mut state = self.state.lock().expect("process state");
            state.loaded_url = Some(url.to_owned());
            state.playhead_seconds = seconds;
            Ok(())
        }

        fn set_paused(&self, is_paused: bool) -> Result<(), String> {
            self.state.lock().expect("process state").is_paused = is_paused;
            Ok(())
        }

        fn seek(&self, seconds: f64) -> Result<(), String> {
            let mut state = self.state.lock().expect("process state");
            state.playhead_seconds = seconds;
            state.standalone_seek_calls += 1;
            Ok(())
        }

        fn set_volume(&self, value: f64) -> Result<(), String> {
            self.state.lock().expect("process state").volume = value;
            Ok(())
        }

        fn set_speed(&self, rate: f64) -> Result<(), String> {
            self.state.lock().expect("process state").playback_rate = rate;
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    impl MpvProcessAdapter for FakeMpvProcess {
        fn load(&self, url: &str) -> Result<(), String> {
            if let Some(error) = self.load_error.as_ref() {
                return Err(error.clone());
            }
            *self.loaded_url.lock().expect("loaded URL") = Some(url.to_owned());
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {
            self.is_shutdown.store(true, Ordering::Release);
        }
    }

    impl MpvProcessAdapter for ReconnectMpvProcess {
        fn load(&self, _url: &str) -> Result<(), String> {
            self.load_calls.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn set_paused(&self, _is_paused: bool) -> Result<(), String> {
            Ok(())
        }

        fn seek(&self, _seconds: f64) -> Result<(), String> {
            Ok(())
        }

        fn set_volume(&self, _value: f64) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    fn publish_current_load_boundary(
        controller: &PlaybackController,
        event_sender: &std::sync::mpsc::Sender<MpvEvent>,
    ) {
        let load_revision = controller.load_revision.load(Ordering::Acquire);
        event_sender
            .send(MpvEvent::LoadBoundary(load_revision))
            .expect("current load boundary");
    }

    #[test]
    fn track_playback_projects_mpv_timing_events() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let loaded_url = Arc::new(Mutex::new(None));
        let is_shutdown = Arc::new(AtomicBool::new(false));
        let process = FakeMpvProcess {
            loaded_url: loaded_url.clone(),
            is_shutdown,
            load_error: None,
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let source = json!({
            "type": "track",
            "track": {
                "id": "track-1",
                "title": "Track 1",
                "durationMs": 120000
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });

        let playing = controller.play(Some(source.clone())).expect("play Track");
        event_sender.send(MpvEvent::Time(15.5)).expect("time event");
        event_sender
            .send(MpvEvent::Duration(121.25))
            .expect("duration event");
        let observed = wait_for_state(&controller, |state| {
            state.current_time == 15.5 && state.duration == 121.25
        });

        assert_eq!(playing.status, PlaybackStatus::Playing);
        assert_eq!(playing.source, Some(source));
        assert_eq!(playing.duration, 120.0);
        assert_eq!(observed.current_time, 15.5);
        assert_eq!(observed.duration, 121.25);
        assert_eq!(
            loaded_url.lock().expect("loaded URL").as_deref(),
            Some("http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream")
        );
    }

    #[test]
    fn adaptive_output_mode_enables_without_a_confirmation_payload() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let adaptive = AdaptiveSystemRateController::new(Arc::new(RecordedPipeWireRate::default()));
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );

        let enabled = controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        assert_eq!(enabled.output_mode, OutputMode::AdaptiveSystemRate);
    }

    #[test]
    fn adaptive_cleanup_marker_failure_returns_a_stable_public_message() {
        let (_event_sender, events) = std::sync::mpsc::channel();
        let adaptive = AdaptiveSystemRateController::with_cleanup_marker(
            Arc::new(RecordedPipeWireRate::default()),
            Arc::new(FailingCleanupMarker),
        );
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            events,
            adaptive,
            |_| {},
        );

        let error = controller
            .enable_adaptive_system_rate()
            .expect_err("surface marker persistence failure");

        assert!(error.message.contains("storage permissions"));
        assert!(!error.message.contains("private-user"));
        assert!(!error.message.contains("secret-marker"));
    }

    #[test]
    fn adaptive_output_applies_each_playback_source_sample_rate() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");

        for (id, sample_rate_hz) in [("track-1", 44_100), ("track-2", 96_000)] {
            controller
                .play(Some(json!({
                    "type": "track",
                    "track": {
                        "id": id,
                        "title": id,
                        "sampleRateHz": sample_rate_hz,
                        "bitDepth": 24,
                        "bitrateKbps": 4608
                    },
                    "playbackUrl": format!(
                        "http://127.0.0.1:43129/token/api/v1/tracks/{id}/stream"
                    )
                })))
                .expect("play Track");
            assert_eq!(
                pipewire_rate.forced_rate_hz().expect("observe force-rate"),
                Some(sample_rate_hz)
            );
        }
    }

    #[test]
    fn adaptive_output_uses_decoder_rate_when_live_source_metadata_is_unknown() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "radio-station",
                "station": { "id": "station-1", "name": "Station 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-1/stream"
            })))
            .expect("play Radio Station");

        event_sender
            .send(MpvEvent::Decoder(ObservedMpvProperties {
                pcm_format: Some("s16".to_owned()),
                format: AudioFormatObservation {
                    sample_rate_hz: Some(48_000),
                    bit_depth: Some(16),
                    channels: Some(2),
                },
            }))
            .expect("decoder event");

        wait_for_state(&controller, |state| {
            state.telemetry.decoder.format.sample_rate_hz == Some(48_000)
        });
        assert_eq!(
            pipewire_rate.forced_rate_hz().expect("observe force-rate"),
            Some(48_000)
        );
    }

    #[test]
    fn explicit_shutdown_restores_automatic_system_rate() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 192000 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        controller.shutdown().expect("shutdown native playback");

        assert_eq!(
            pipewire_rate
                .forced_rate_hz()
                .expect("observe automatic rate"),
            None
        );
    }

    #[test]
    fn explicit_quit_does_not_exit_when_system_rate_cleanup_fails() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let adaptive =
            AdaptiveSystemRateController::new(Arc::new(FailingResetPipeWireRate::default()));
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");
        let snapshot_path = temporary_path("cleanup-failure-snapshot.json");
        let shell = TestDesktopPlaybackShell::default();
        let lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));

        let error = dispatch_desktop_playback_action(
            DesktopPlaybackAction::Quit,
            &controller,
            &lifecycle,
            &PlaybackSnapshotStore::new(snapshot_path.clone()),
            &shell,
        )
        .expect_err("surface safety-critical cleanup failure");

        assert!(error.message.contains("could not be disabled"));
        assert!(!shell.has_exited.load(Ordering::Acquire));
        assert!(!snapshot_path.exists());
    }

    #[test]
    fn disabling_adaptive_output_restores_automatic_system_rate() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 88200 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        let system = controller
            .fallback_to_system_output()
            .expect("restore System Output");

        assert_eq!(system.output_mode, OutputMode::System);
        assert_eq!(
            pipewire_rate
                .forced_rate_hz()
                .expect("observe automatic rate"),
            None
        );
    }

    #[test]
    fn adaptive_output_cleans_up_when_mpv_setup_fails() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: Some("controlled mpv setup failure".to_owned()),
        };
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(process),
            event_receiver,
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");

        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 176400 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect_err("surface mpv setup failure");

        assert_eq!(
            pipewire_rate
                .forced_rate_hz()
                .expect("observe automatic rate"),
            None
        );
    }

    #[test]
    fn player_recovery_restores_automatic_rate_before_resuming_adaptive_output() {
        let (initial_event_sender, initial_events) = std::sync::mpsc::channel();
        let (recovery_event_sender, recovery_events) = std::sync::mpsc::channel();
        let recovery_events = Arc::new(Mutex::new(Some(recovery_events)));
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let adaptive = AdaptiveSystemRateController::new(pipewire_rate.clone());
        let controller = PlaybackController::start_recoverable_with_adaptive_system_rate(
            Box::new(RestorableMpvProcess {
                state: Arc::new(Mutex::new(RestoredProcessState::default())),
            }),
            initial_events,
            Arc::new(Mutex::new(PlaybackLifecycle::new())),
            {
                let recovery_events = recovery_events.clone();
                move || {
                    Ok((
                        Box::new(RestorableMpvProcess {
                            state: Arc::new(Mutex::new(RestoredProcessState::default())),
                        }) as Box<dyn MpvProcessAdapter>,
                        recovery_events
                            .lock()
                            .expect("recovery events")
                            .take()
                            .ok_or_else(|| "recovery budget exhausted".to_owned())?,
                    ))
                }
            },
            adaptive,
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        initial_event_sender
            .send(MpvEvent::ExitedUnexpectedly("controlled crash".to_owned()))
            .expect("crash event");
        wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Playing
                && pipewire_rate.has_restored_automatic.load(Ordering::Acquire)
        });

        assert!(pipewire_rate.has_restored_automatic.load(Ordering::Acquire));
        assert_eq!(
            pipewire_rate
                .forced_rate_hz()
                .expect("observe recovered force-rate"),
            Some(96_000)
        );
        drop(recovery_event_sender);
    }

    #[test]
    fn failed_player_recovery_clears_the_reapplied_adaptive_rate() {
        let (initial_event_sender, initial_events) = std::sync::mpsc::channel();
        let (_recovery_event_sender, recovery_events) = std::sync::mpsc::channel();
        let recovery_events = Arc::new(Mutex::new(Some(recovery_events)));
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let controller = PlaybackController::start_recoverable_with_adaptive_system_rate(
            Box::new(RestorableMpvProcess {
                state: Arc::new(Mutex::new(RestoredProcessState::default())),
            }),
            initial_events,
            Arc::new(Mutex::new(PlaybackLifecycle::new())),
            {
                let recovery_events = recovery_events.clone();
                move || {
                    Ok((
                        Box::new(FakeMpvProcess {
                            loaded_url: Arc::new(Mutex::new(None)),
                            is_shutdown: Arc::new(AtomicBool::new(false)),
                            load_error: Some("controlled recovery setup failure".to_owned()),
                        }) as Box<dyn MpvProcessAdapter>,
                        recovery_events
                            .lock()
                            .expect("recovery events")
                            .take()
                            .ok_or_else(|| "recovery budget exhausted".to_owned())?,
                    ))
                }
            },
            AdaptiveSystemRateController::new(pipewire_rate.clone()),
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable Adaptive System Rate");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        initial_event_sender
            .send(MpvEvent::ExitedUnexpectedly("controlled crash".to_owned()))
            .expect("crash event");
        wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(
            pipewire_rate.forced_rate_hz().expect("observe safe rate"),
            None
        );
    }

    #[test]
    fn failed_automatic_queue_advance_clears_the_adaptive_rate() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FailingAdvanceMpv {
                load_calls: AtomicUsize::new(0),
            }),
            events,
            AdaptiveSystemRateController::new(pipewire_rate.clone()),
            |_| {},
        );
        let sources = vec![
            json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 44100 },
                "playbackUrl": "http://127.0.0.1/token/api/v1/tracks/track-1/stream"
            }),
            json!({
                "type": "track",
                "track": { "id": "track-2", "title": "Track 2", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1/token/api/v1/tracks/track-2/stream"
            }),
        ];
        controller
            .sync_queue_context(sources.clone(), Some(0))
            .expect("sync queue");
        controller
            .enable_adaptive_system_rate()
            .expect("enable adaptive output");
        controller
            .play(Some(sources[0].clone()))
            .expect("play first source");

        event_sender.send(MpvEvent::Ended).expect("ended event");
        wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(pipewire_rate.forced_rate_hz().expect("safe rate"), None);
    }

    #[test]
    fn mpv_error_event_clears_the_adaptive_rate() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            events,
            AdaptiveSystemRateController::new(pipewire_rate.clone()),
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable adaptive output");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 88200 },
                "playbackUrl": "http://127.0.0.1/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play source");

        event_sender
            .send(MpvEvent::Error("controlled decoder failure".to_owned()))
            .expect("error event");
        wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(pipewire_rate.forced_rate_hz().expect("safe rate"), None);
    }

    #[test]
    fn repeat_once_failure_clears_the_adaptive_rate() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let pipewire_rate = Arc::new(RecordedPipeWireRate::default());
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FailingRepeatMpv),
            events,
            AdaptiveSystemRateController::new(pipewire_rate.clone()),
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable adaptive output");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play source");
        controller.cycle_repeat_mode().expect("enable Repeat Once");

        event_sender.send(MpvEvent::Ended).expect("ended event");
        wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(pipewire_rate.forced_rate_hz().expect("safe rate"), None);
    }

    #[test]
    fn repeat_once_cleanup_failure_surfaces_stable_context() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let controller = PlaybackController::start_with_adaptive_system_rate(
            Box::new(FailingRepeatMpv),
            events,
            AdaptiveSystemRateController::new(Arc::new(FailingResetPipeWireRate::default())),
            |_| {},
        );
        controller
            .enable_adaptive_system_rate()
            .expect("enable adaptive output");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "sampleRateHz": 96000 },
                "playbackUrl": "http://127.0.0.1/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play source");
        controller.cycle_repeat_mode().expect("enable Repeat Once");

        event_sender.send(MpvEvent::Ended).expect("ended event");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);
        let message = &failed.error.expect("cleanup error").message;

        assert!(message.contains("could not reset the system audio rate"));
        assert!(!message.contains("controlled PipeWire"));
    }

    #[test]
    fn replacing_or_stopping_a_source_clears_stale_path_observations() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let first_source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });
        let next_source = json!({
            "type": "track",
            "track": { "id": "track-2", "title": "Track 2", "durationMs": 90000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-2/stream"
        });

        controller
            .play(Some(first_source))
            .expect("play first Track");
        event_sender
            .send(MpvEvent::Decoder(ObservedMpvProperties::from_audio_params(
                &json!({ "format": "s24", "samplerate": 96000, "channel-count": 2 }),
            )))
            .expect("decoder event");
        wait_for_state(&controller, |state| {
            state.telemetry.decoder.format.sample_rate_hz == Some(96_000)
        });

        let replaced = controller
            .play(Some(next_source))
            .expect("play replacement Track");
        assert_eq!(replaced.telemetry.decoder, ObservedMpvProperties::unknown());

        let stopped = controller.stop().expect("stop playback");
        assert_eq!(stopped.telemetry.source, SourceObservation::unknown());
        assert_eq!(stopped.telemetry.decoder, ObservedMpvProperties::unknown());
        assert_eq!(stopped.telemetry.system.format.sample_rate_hz, None);
        assert_eq!(stopped.telemetry.device.name, None);
    }

    #[test]
    fn pipewire_path_is_sampled_after_decoder_readiness_and_refreshed_during_playback() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let observer = Arc::new(ObservedPipeWirePath {
            calls: AtomicUsize::new(0),
        });
        let controller = PlaybackController::start_with_path_observer(
            Box::new(process),
            event_receiver,
            observer.clone(),
            |_| {},
        );
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });

        let loading = controller.play(Some(source)).expect("load Track");
        assert_eq!(observer.calls.load(Ordering::Relaxed), 0);
        assert_eq!(loading.telemetry.device.name, None);

        event_sender
            .send(MpvEvent::Decoder(ObservedMpvProperties::from_audio_params(
                &json!({ "format": "s24", "samplerate": 96000, "channel-count": 2 }),
            )))
            .expect("decoder readiness");
        let observed = wait_for_state(&controller, |state| {
            state.telemetry.device.name.as_deref() == Some("Routed USB DAC")
        });

        assert_eq!(observer.calls.load(Ordering::Relaxed), 1);
        assert_eq!(observed.telemetry.system.format.bit_depth, Some(24));

        thread::sleep(super::PATH_REFRESH_INTERVAL + Duration::from_millis(10));
        event_sender.send(MpvEvent::Time(1.0)).expect("time event");
        wait_for_state(&controller, |_| observer.calls.load(Ordering::Relaxed) == 2);
        assert_eq!(observer.calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn processing_state_reports_effective_mpv_results_without_changing_output_mode() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let requested = Arc::new(Mutex::new(None));
        let controller = PlaybackController::start(
            Box::new(ObservedProcessingMpv {
                requested: requested.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let mut processing = ProcessingState::default();
        processing.profile = crate::processing::ProcessingProfile::Processed;
        processing.software_volume = 0.4;
        processing.replay_gain_mode = ReplayGainMode::Album;
        processing.equalizer.is_enabled = true;
        let configuration = MpvProcessingConfiguration {
            volume_percent: 40.0,
            replay_gain_mode: ReplayGainMode::Album,
            audio_filters: vec!["lavfi=[equalizer=f=1000:g=2]".to_owned()],
        };
        let output_mode = controller.state().expect("initial state").output_mode;
        assert_eq!(output_mode, OutputMode::System);

        let observed = controller
            .apply_processing(processing, &configuration)
            .expect("apply processing");

        assert_eq!(
            *requested.lock().expect("requested processing"),
            Some(configuration)
        );
        assert_eq!(observed.processing.software_volume, 0.37);
        assert_eq!(observed.processing.replay_gain_mode, ReplayGainMode::Track);
        assert!(observed.processing.equalizer.is_enabled);
        assert_eq!(
            observed.processing.effective_audio_filters,
            ["lavfi=[equalizer=f=1000:g=1.5]"]
        );
        assert_eq!(observed.telemetry.processing.software_volume, Some(0.37));
        assert_eq!(observed.output_mode, output_mode);
    }

    #[test]
    fn direct_alsa_selection_uses_raw_hardware_and_reports_pipewire_bypass() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let configured = Arc::new(Mutex::new(Vec::new()));
        let adapter = FakeOutputDevices {
            devices: Arc::new(Mutex::new(vec![OutputDevice {
                id: "hw:2,0".to_owned(),
                name: "USB DAC".to_owned(),
            }])),
        };
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: configured.clone(),
                is_paused: Arc::new(AtomicBool::new(false)),
                direct_configuration_error: None,
                negotiated_format: Some(AudioFormatObservation {
                    sample_rate_hz: Some(96_000),
                    bit_depth: Some(24),
                    channels: Some(2),
                }),
            }),
            events,
            Box::new(adapter),
            |_| {},
        );

        let listed = controller
            .refresh_output_devices()
            .expect("list raw devices");
        let direct = controller
            .select_direct_alsa_output("hw:2,0")
            .expect("select Direct ALSA Output");

        assert_eq!(listed.available_output_devices.len(), 1);
        assert_eq!(direct.output_mode, OutputMode::DirectAlsa);
        assert_eq!(
            direct
                .selected_output_device
                .as_ref()
                .map(|device| device.id.as_str()),
            Some("hw:2,0")
        );
        assert_eq!(
            direct.telemetry.system.kind,
            crate::telemetry::SystemObservationKind::Bypassed
        );
        assert_eq!(
            configured.lock().expect("output configurations").as_slice(),
            [MpvOutputConfiguration::DirectAlsa {
                device_id: "hw:2,0".to_owned()
            }]
        );
        event_sender
            .send(MpvEvent::Decoder(ObservedMpvProperties::from_audio_params(
                &json!({ "format": "s24", "samplerate": 96000, "channel-count": 2 }),
            )))
            .expect("decoder event");
        let decoded = wait_for_state(&controller, |state| {
            state.telemetry.decoder.format.sample_rate_hz == Some(96_000)
        });
        assert_eq!(decoded.telemetry.device.format.sample_rate_hz, Some(96_000));
    }

    #[test]
    fn real_mpv_reserves_raw_alsa_before_switching_and_releases_it_for_system_output() {
        assert_eq!(
            record_real_mpv_output_actions(&[
                MpvOutputConfiguration::DirectAlsa {
                    device_id: "hw:2,0".to_owned(),
                },
                MpvOutputConfiguration::System,
            ]),
            [
                "mpv:[\"set_property\",\"aid\",\"no\"]",
                "reserve:hw:2,0",
                "mpv:[\"set_property\",\"audio-device\",\"alsa/hw:2,0\"]",
                "mpv:[\"set_property\",\"aid\",\"auto\"]",
                "mpv:[\"set_property\",\"aid\",\"no\"]",
                "mpv:[\"set_property\",\"audio-device\",\"auto\"]",
                "release",
                "mpv:[\"set_property\",\"aid\",\"auto\"]",
            ]
        );
        assert_eq!(
            record_real_mpv_output_actions(&[MpvOutputConfiguration::DirectAlsa {
                device_id: "hw:2,0".to_owned(),
            }]),
            [
                "mpv:[\"set_property\",\"aid\",\"no\"]",
                "reserve:hw:2,0",
                "mpv:[\"set_property\",\"audio-device\",\"alsa/hw:2,0\"]",
                "mpv:[\"set_property\",\"aid\",\"auto\"]",
                "release",
            ]
        );
    }

    #[test]
    fn direct_alsa_reobserves_the_negotiated_format_for_each_new_source() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let observed_format = Arc::new(Mutex::new(Some(AudioFormatObservation {
            sample_rate_hz: Some(192_000),
            bit_depth: Some(24),
            channels: Some(2),
        })));
        let observation_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start_with_output_devices(
            Box::new(ReobservedOutputMpv {
                configured_format: Some(AudioFormatObservation {
                    sample_rate_hz: Some(44_100),
                    bit_depth: Some(16),
                    channels: Some(2),
                }),
                observed_format,
                observation_calls: observation_calls.clone(),
            }),
            events,
            Box::new(FakeOutputDevices {
                devices: Arc::new(Mutex::new(vec![OutputDevice {
                    id: "hw:2,0".to_owned(),
                    name: "USB DAC".to_owned(),
                }])),
            }),
            |_| {},
        );
        controller
            .refresh_output_devices()
            .expect("list raw devices");
        controller
            .select_direct_alsa_output("hw:2,0")
            .expect("select Direct ALSA Output");

        let playing = controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play new source");

        assert_eq!(observation_calls.load(Ordering::Acquire), 0);
        assert_eq!(playing.telemetry.device.format.sample_rate_hz, None,);
        event_sender
            .send(MpvEvent::Decoder(ObservedMpvProperties::from_audio_params(
                &json!({ "format": "s24", "samplerate": 192000, "channel-count": 2 }),
            )))
            .expect("audio readiness event");
        let ready = wait_for_state(&controller, |state| {
            state.telemetry.device.format.sample_rate_hz == Some(192_000)
        });

        assert_eq!(observation_calls.load(Ordering::Acquire), 1);
        assert_eq!(ready.telemetry.device.format.sample_rate_hz, Some(192_000));
    }

    #[test]
    fn direct_alsa_negotiated_format_survives_periodic_path_refresh() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let controller = PlaybackController::start_with_output_devices_and_path_observer(
            Box::new(OutputConfiguredMpv {
                configured: Arc::new(Mutex::new(Vec::new())),
                is_paused: Arc::new(AtomicBool::new(false)),
                direct_configuration_error: None,
                negotiated_format: Some(AudioFormatObservation {
                    sample_rate_hz: Some(96_000),
                    bit_depth: Some(24),
                    channels: Some(2),
                }),
            }),
            events,
            Arc::new(ObservedPipeWirePath {
                calls: AtomicUsize::new(0),
            }),
            Box::new(FakeOutputDevices {
                devices: Arc::new(Mutex::new(vec![OutputDevice {
                    id: "hw:2,0".to_owned(),
                    name: "USB DAC".to_owned(),
                }])),
            }),
            |_| {},
        );
        controller
            .refresh_output_devices()
            .expect("list raw devices");
        controller
            .select_direct_alsa_output("hw:2,0")
            .expect("select Direct ALSA Output");

        thread::sleep(PATH_REFRESH_INTERVAL + Duration::from_millis(20));
        event_sender.send(MpvEvent::Time(1.0)).expect("time event");
        let refreshed = wait_for_state(&controller, |state| state.current_time == 1.0);

        assert_eq!(
            refreshed.telemetry.device.format.sample_rate_hz,
            Some(96_000)
        );
    }

    #[test]
    fn output_device_enumeration_does_not_publish_internal_details() {
        let (_event_sender, events) = std::sync::mpsc::channel();
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: Arc::new(Mutex::new(Vec::new())),
                is_paused: Arc::new(AtomicBool::new(false)),
                direct_configuration_error: None,
                negotiated_format: None,
            }),
            events,
            Box::new(FailingOutputDevices),
            |_| {},
        );

        let error = controller
            .refresh_output_devices()
            .expect_err("surface stable enumeration error");

        assert_eq!(
            error.message,
            "Raw ALSA Output Devices could not be listed. Check the local ALSA setup and try again."
        );
        assert!(!error.message.contains("private-user"));
        assert!(!error.message.contains("secret-device-token"));
    }

    #[test]
    fn direct_alsa_failure_requires_explicit_system_fallback() {
        let (_event_sender, events) = std::sync::mpsc::channel();
        let adapter = FakeOutputDevices {
            devices: Arc::new(Mutex::new(vec![OutputDevice {
                id: "hw:2,0".to_owned(),
                name: "USB DAC".to_owned(),
            }])),
        };
        let configured = Arc::new(Mutex::new(Vec::new()));
        let is_paused = Arc::new(AtomicBool::new(false));
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: configured.clone(),
                is_paused: is_paused.clone(),
                direct_configuration_error: Some("device or resource busy".to_owned()),
                negotiated_format: None,
            }),
            events,
            Box::new(adapter),
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track before selecting Direct ALSA Output");

        let prompted = controller
            .select_direct_alsa_output("hw:2,0")
            .expect("publish actionable prompt");

        assert_eq!(prompted.output_mode, OutputMode::System);
        assert_eq!(prompted.status, PlaybackStatus::Paused);
        assert!(is_paused.load(Ordering::Acquire));
        assert_eq!(
            prompted
                .output_device_issue
                .as_ref()
                .map(|issue| issue.code.as_str()),
            Some("busy-or-unsupported")
        );
        assert!(prompted.output_device_issue.is_some_and(|issue| {
            issue.message.contains("Normal Output") && issue.message.contains("system output")
        }));
        assert_eq!(
            configured.lock().expect("output configurations").last(),
            Some(&MpvOutputConfiguration::System)
        );
    }

    #[test]
    fn exclusive_selection_waits_for_real_output_before_publishing() {
        let (_event_sender, events) = std::sync::mpsc::channel();
        let configured = Arc::new(Mutex::new(Vec::new()));
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: configured.clone(),
                is_paused: Arc::new(AtomicBool::new(false)),
                direct_configuration_error: None,
                negotiated_format: None,
            }),
            events,
            Box::new(FakeOutputDevices {
                devices: Arc::new(Mutex::new(vec![OutputDevice {
                    id: "hw:2,0".to_owned(),
                    name: "USB DAC".to_owned(),
                }])),
            }),
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track before selecting Exclusive Output");

        let prompted = controller
            .select_direct_alsa_output("hw:2,0")
            .expect("restore System Output when Exclusive is not ready");

        assert_eq!(prompted.output_mode, OutputMode::System);
        assert_eq!(prompted.status, PlaybackStatus::Paused);
        assert_eq!(
            configured.lock().expect("output configurations").last(),
            Some(&MpvOutputConfiguration::System)
        );
    }

    #[test]
    fn disconnected_direct_dac_pauses_without_silent_fallback() {
        let (_event_sender, events) = std::sync::mpsc::channel();
        let adapter = FakeOutputDevices {
            devices: Arc::new(Mutex::new(vec![OutputDevice {
                id: "hw:2,0".to_owned(),
                name: "USB DAC".to_owned(),
            }])),
        };
        let is_paused = Arc::new(AtomicBool::new(false));
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: Arc::new(Mutex::new(Vec::new())),
                is_paused: is_paused.clone(),
                direct_configuration_error: None,
                negotiated_format: None,
            }),
            events,
            Box::new(adapter.clone()),
            |_| {},
        );
        controller
            .select_direct_alsa_output("hw:2,0")
            .expect("select Direct ALSA Output");
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        adapter.replace(Vec::new());
        let disconnected = wait_for_state(&controller, |state| {
            state
                .output_device_issue
                .as_ref()
                .is_some_and(|issue| issue.code == OutputDeviceIssueCode::Disconnected)
        });

        assert_eq!(disconnected.output_mode, OutputMode::DirectAlsa);
        assert_eq!(disconnected.status, PlaybackStatus::Paused);
        assert!(is_paused.load(Ordering::Acquire));
        assert_eq!(
            disconnected
                .output_device_issue
                .as_ref()
                .map(|issue| issue.code.as_str()),
            Some("disconnected")
        );

        let fallback = controller
            .fallback_to_system_output()
            .expect("confirm System Output fallback");
        assert_eq!(fallback.output_mode, OutputMode::System);
        assert!(fallback.output_device_issue.is_none());
    }

    #[test]
    fn exclusive_output_failure_restores_system_output() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let configured = Arc::new(Mutex::new(Vec::new()));
        let adapter = FakeOutputDevices {
            devices: Arc::new(Mutex::new(vec![OutputDevice {
                id: "hw:2,0".to_owned(),
                name: "USB DAC".to_owned(),
            }])),
        };
        let controller = PlaybackController::start_with_output_devices(
            Box::new(OutputConfiguredMpv {
                configured: configured.clone(),
                is_paused: Arc::new(AtomicBool::new(false)),
                direct_configuration_error: None,
                negotiated_format: None,
            }),
            events,
            Box::new(adapter),
            |_| {},
        );
        controller
            .select_direct_alsa_output("hw:2,0")
            .expect("select Direct ALSA Output");

        event_sender
            .send(MpvEvent::Error("unsupported audio format".to_owned()))
            .expect("mpv format failure");
        let prompted = wait_for_state(&controller, |state| state.output_device_issue.is_some());

        assert_eq!(prompted.output_mode, OutputMode::System);
        assert_eq!(prompted.status, PlaybackStatus::Paused);
        assert_eq!(
            prompted
                .output_device_issue
                .as_ref()
                .map(|issue| issue.code.as_str()),
            Some("busy-or-unsupported")
        );
        assert_eq!(
            configured.lock().expect("output configurations").last(),
            Some(&MpvOutputConfiguration::System)
        );
    }

    #[test]
    fn album_replay_gain_publishes_the_effective_track_fallback() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": {
                    "id": "track-1",
                    "title": "Track 1",
                    "durationMs": 120000,
                    "replayGain": {
                        "trackGainDb": -7.25,
                        "trackPeak": 0.9,
                        "albumGainDb": null,
                        "albumPeak": null
                    }
                },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track with ReplayGain metadata");
        let mut processing = ProcessingState::default();
        processing.profile = crate::processing::ProcessingProfile::Processed;
        processing.replay_gain_mode = ReplayGainMode::Album;

        let observed = controller
            .apply_processing(
                processing,
                &MpvProcessingConfiguration {
                    volume_percent: 100.0,
                    replay_gain_mode: ReplayGainMode::Album,
                    audio_filters: Vec::new(),
                },
            )
            .expect("apply Album ReplayGain");

        assert_eq!(
            observed.processing.effective_replay_gain_mode,
            EffectiveReplayGainMode::TrackFallback
        );
        assert_eq!(
            observed.telemetry.processing.effective_replay_gain_mode,
            "track-fallback"
        );

        let unavailable = controller
            .play(Some(json!({
                "type": "radio-station",
                "station": {
                    "id": "radio-1",
                    "name": "Radio",
                    "streamUrl": "https://example.com/radio"
                },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/radio-1/stream",
                "sourceUrl": "https://example.com/radio"
            })))
            .expect("switch to source without ReplayGain metadata");
        assert_eq!(
            unavailable.processing.effective_replay_gain_mode,
            EffectiveReplayGainMode::Unavailable
        );
        assert_eq!(
            unavailable.telemetry.processing.effective_replay_gain_mode,
            "unavailable"
        );
    }

    #[test]
    fn persisted_session_restores_source_and_playhead_paused() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process_state = Arc::new(Mutex::new(RestoredProcessState::default()));
        let process = RestorableMpvProcess {
            state: process_state.clone(),
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });
        let snapshot = PlaybackSessionSnapshot::new(Some(source.clone()), 41.5, 0.65, true, "loop")
            .expect("valid snapshot");

        let restored = controller
            .restore_paused(&snapshot)
            .expect("restore paused session");
        let process = process_state.lock().expect("process state");

        assert_eq!(restored.source, Some(source));
        assert_eq!(restored.status, PlaybackStatus::Paused);
        assert_eq!(restored.current_time, 41.5);
        assert_eq!(restored.volume, 0.65);
        assert!(restored.shuffle_enabled);
        assert_eq!(restored.repeat_mode, super::RepeatMode::Loop);
        assert!(process.is_paused);
        assert_eq!(process.playhead_seconds, 41.5);
        assert_eq!(process.standalone_seek_calls, 0);
        assert_eq!(process.volume, 0.65);
    }

    #[test]
    fn paused_snapshot_restore_does_not_seek_before_async_load_is_ready() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller =
            PlaybackController::start(Box::new(AsynchronousLoadMpvProcess), event_receiver, |_| {});
        let snapshot = PlaybackSessionSnapshot::new(
            Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })),
            41.5,
            0.65,
            false,
            "off",
        )
        .expect("valid snapshot");

        controller
            .restore_paused(&snapshot)
            .expect("restore paused snapshot without an immediate seek");
    }

    #[test]
    fn player_crash_recovers_once_then_surfaces_an_actionable_error() {
        let (initial_event_sender, initial_events) = std::sync::mpsc::channel();
        let initial_process_state = Arc::new(Mutex::new(RestoredProcessState::default()));
        let initial_process = RestorableMpvProcess {
            state: initial_process_state,
        };
        let (recovery_event_sender, recovery_events) = std::sync::mpsc::channel();
        let recovery_events = Arc::new(Mutex::new(Some(recovery_events)));
        let recovered_process_state = Arc::new(Mutex::new(RestoredProcessState::default()));
        let starter_calls = Arc::new(Mutex::new(0_u8));
        let lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
        let controller = PlaybackController::start_recoverable(
            Box::new(initial_process),
            initial_events,
            lifecycle,
            {
                let recovery_events = recovery_events.clone();
                let process_state = recovered_process_state.clone();
                let starter_calls = starter_calls.clone();
                move || {
                    *starter_calls.lock().expect("starter calls") += 1;
                    let events = recovery_events
                        .lock()
                        .expect("recovery events")
                        .take()
                        .ok_or_else(|| "recovery budget exhausted".to_owned())?;
                    Ok((
                        Box::new(RestorableMpvProcess {
                            state: process_state.clone(),
                        }) as Box<dyn MpvProcessAdapter>,
                        events,
                    ))
                }
            },
            |_| {},
        );
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });
        controller.play(Some(source.clone())).expect("play Track");
        controller.seek(27.5).expect("seek Track");
        controller
            .set_playback_rate(1.5)
            .expect("set playback speed");

        initial_event_sender
            .send(MpvEvent::ExitedUnexpectedly("fixture crash".to_owned()))
            .expect("first crash");
        let recovered = wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Playing
                && state.current_time == 27.5
                && *starter_calls.lock().expect("starter calls") == 1
                && recovered_process_state
                    .lock()
                    .expect("recovered process")
                    .playback_rate
                    == 1.5
        });

        assert_eq!(recovered.source, Some(source));
        assert_eq!(recovered.playback_rate, 1.5);
        assert_eq!(*starter_calls.lock().expect("starter calls"), 1);
        assert!(
            !recovered_process_state
                .lock()
                .expect("recovered process")
                .is_paused
        );

        recovery_event_sender
            .send(MpvEvent::ExitedUnexpectedly(
                "fixture crash again".to_owned(),
            ))
            .expect("second crash");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(*starter_calls.lock().expect("starter calls"), 1);
        assert!(
            failed
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("Quit and reopen the Desktop Client"))
        );
    }

    #[test]
    fn player_crash_without_a_restart_factory_publishes_an_actionable_error() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(RestorableMpvProcess {
                state: Arc::new(Mutex::new(RestoredProcessState::default())),
            }),
            events,
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        event_sender
            .send(MpvEvent::ExitedUnexpectedly("fixture crash".to_owned()))
            .expect("crash event");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(
            failed.error.as_ref().map(|error| error.code.as_str()),
            Some("mpv-restart-unavailable")
        );
        assert!(
            failed
                .error
                .is_some_and(|error| error.message.contains("Restart the Desktop Client"))
        );
    }

    #[test]
    fn restart_internal_details_are_not_published_in_the_playback_session() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
        let controller = PlaybackController::start_recoverable(
            Box::new(RestorableMpvProcess {
                state: Arc::new(Mutex::new(RestoredProcessState::default())),
            }),
            events,
            lifecycle,
            || Err("private IPC /tmp/secret.sock rejected token abc123".to_owned()),
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");

        event_sender
            .send(MpvEvent::ExitedUnexpectedly(
                "private child status".to_owned(),
            ))
            .expect("crash event");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);
        let error = failed.error.expect("actionable recovery error");

        assert_eq!(error.code, "mpv-restart-failed");
        assert_eq!(
            error.message,
            "Native playback could not restart. Quit and reopen the Desktop Client to try again."
        );
        assert!(!error.message.contains("secret"));
        assert!(!error.message.contains("token"));
    }

    #[test]
    fn poisoned_recovery_lifecycle_publishes_an_actionable_error() {
        let (event_sender, events) = std::sync::mpsc::channel();
        let lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
        let controller = PlaybackController::start_recoverable(
            Box::new(RestorableMpvProcess {
                state: Arc::new(Mutex::new(RestoredProcessState::default())),
            }),
            events,
            lifecycle.clone(),
            || Err("restart must not be attempted".to_owned()),
            |_| {},
        );
        controller
            .play(Some(json!({
                "type": "track",
                "track": { "id": "track-1", "title": "Track 1" },
                "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
            })))
            .expect("play Track");
        let poisoned_lifecycle = lifecycle.clone();
        assert!(
            thread::spawn(move || {
                let _guard = poisoned_lifecycle.lock().expect("lifecycle");
                panic!("poison lifecycle fixture");
            })
            .join()
            .is_err()
        );

        event_sender
            .send(MpvEvent::ExitedUnexpectedly("fixture crash".to_owned()))
            .expect("crash event");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(
            failed.error.as_ref().map(|error| error.code.as_str()),
            Some("mpv-recovery-lifecycle-unavailable")
        );
        assert!(
            failed
                .error
                .is_some_and(|error| error.message.contains("Restart the Desktop Client"))
        );
    }

    #[test]
    fn radio_and_catalog_sources_load_proxy_urls_as_continuous_streams() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let loaded_url = Arc::new(Mutex::new(None));
        let process = FakeMpvProcess {
            loaded_url: loaded_url.clone(),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let radio_source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-1",
                "name": "Controlled MP3",
                "streamUrl": "https://radio.example/live.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-1/stream",
            "sourceUrl": "https://radio.example/live.mp3"
        });
        let catalog_source = json!({
            "type": "catalog-preview",
            "result": {
                "stationUuid": "catalog-1",
                "name": "Controlled HLS",
                "streamUrl": "https://catalog.example/live.m3u8"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/preview/catalog-1/stream",
            "sourceUrl": "https://catalog.example/live.m3u8"
        });

        let radio = controller
            .play(Some(radio_source.clone()))
            .expect("play saved Radio Station");
        assert_eq!(radio.source, Some(radio_source));
        assert_eq!(radio.status, PlaybackStatus::Playing);
        assert_eq!(radio.duration, 0.0);
        assert_eq!(
            loaded_url.lock().expect("loaded URL").as_deref(),
            Some("http://127.0.0.1:43129/token/api/v1/radio/stations/station-1/stream")
        );

        let preview = controller
            .play(Some(catalog_source.clone()))
            .expect("play Catalog Preview");
        assert_eq!(preview.source, Some(catalog_source));
        assert_eq!(preview.status, PlaybackStatus::Playing);
        assert_eq!(preview.duration, 0.0);
        assert_eq!(
            loaded_url.lock().expect("loaded URL").as_deref(),
            Some("http://127.0.0.1:43129/token/api/v1/radio/preview/catalog-1/stream")
        );
    }

    #[test]
    fn controls_ended_and_errors_flow_through_native_state() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let is_shutdown = Arc::new(AtomicBool::new(false));
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: is_shutdown.clone(),
            load_error: None,
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });

        controller.play(Some(source)).expect("play Track");
        controller.pause().expect("pause Track");
        controller.seek(25.0).expect("seek Track");
        event_sender.send(MpvEvent::Ended).expect("ended event");
        let ended = wait_for_state(&controller, |state| state.status == PlaybackStatus::Ended);
        event_sender
            .send(MpvEvent::Error("decoder failed".to_owned()))
            .expect("error event");
        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);

        assert_eq!(ended.current_time, 25.0);
        assert_eq!(
            failed.error.as_ref().map(|error| error.message.as_str()),
            Some("Playback failed. Check the source and try again.")
        );
        drop(controller);
        assert!(is_shutdown.load(Ordering::Acquire));
    }

    #[test]
    fn started_live_radio_enters_reconnecting_when_the_stream_ends() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: None,
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-reconnect",
                "name": "Reconnect Radio",
                "streamUrl": "https://radio.example/live.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-reconnect/stream",
            "sourceUrl": "https://radio.example/live.mp3"
        });

        controller
            .play(Some(source))
            .expect("play live Radio Station");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("stable live event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        event_sender
            .send(MpvEvent::Ended)
            .expect("ended live event");
        let reconnecting =
            wait_for_state(&controller, |state| state.status != PlaybackStatus::Playing);

        assert_eq!(format!("{:?}", reconnecting.status), "Reconnecting");
    }

    #[test]
    fn live_radio_reloads_after_the_first_reconnect_delay() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: load_calls.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-reload",
                "name": "Reload Radio",
                "streamUrl": "https://radio.example/live.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-reload/stream",
            "sourceUrl": "https://radio.example/live.mp3"
        });

        controller
            .play(Some(source))
            .expect("play live Radio Station");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("stable live event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        event_sender
            .send(MpvEvent::Ended)
            .expect("ended live event");
        wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Reconnecting
        });

        let deadline = Instant::now() + Duration::from_millis(250);
        while load_calls.load(Ordering::Acquire) < 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(load_calls.load(Ordering::Acquire), 2);
        assert_eq!(
            controller.state().expect("reconnecting state").status,
            PlaybackStatus::Reconnecting
        );
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(2.0))
            .expect("recovered live event");
        let recovered = wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Playing && state.current_time == 2.0
        });
        assert_eq!(recovered.status, PlaybackStatus::Playing);
    }

    #[test]
    fn stable_live_events_do_not_push_the_backoff_reset_deadline_forward() {
        let started_at = Instant::now();
        let mut live_reconnect = LiveReconnectState {
            attempt: 2,
            ..LiveReconnectState::default()
        };

        arm_live_reconnect_stability(&mut live_reconnect, started_at);
        let first_deadline = live_reconnect
            .stability_deadline
            .expect("stability deadline");
        arm_live_reconnect_stability(&mut live_reconnect, started_at + Duration::from_millis(50));

        assert_eq!(live_reconnect.stability_deadline, Some(first_deadline));
        let playing = PlaybackSessionState {
            status: PlaybackStatus::Playing,
            ..PlaybackSessionState::default()
        };
        assert!(update_live_reconnect_stability(
            &mut live_reconnect,
            &playing,
            first_deadline,
        ));
        assert_eq!(live_reconnect.attempt, 0);
    }

    #[test]
    fn stale_live_reconnect_rechecks_revision_after_the_process_lock() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: load_calls.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-stale-reconnect",
                "name": "Stale Reconnect Radio",
                "streamUrl": "https://radio.example/stale.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-stale-reconnect/stream",
            "sourceUrl": "https://radio.example/stale.mp3"
        });

        controller
            .play(Some(source))
            .expect("play live Radio Station");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("stable live event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        event_sender
            .send(MpvEvent::Ended)
            .expect("ended live event");
        wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Reconnecting
        });

        let process = controller.process.clone();
        let process_guard = process.lock().expect("hold process lock");
        thread::sleep(Duration::from_millis(75));
        controller.command_revision.fetch_add(1, Ordering::AcqRel);
        drop(process_guard);
        thread::sleep(Duration::from_millis(75));

        assert_eq!(load_calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn started_catalog_preview_reconnects_after_a_direct_alsa_playback_error() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: Arc::new(AtomicUsize::new(0)),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "catalog-preview",
            "result": {
                "stationUuid": "catalog-reconnect",
                "name": "Reconnect Catalog",
                "streamUrl": "https://radio.example/catalog.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/preview/catalog-reconnect/stream",
            "sourceUrl": "https://radio.example/catalog.mp3"
        });

        controller.play(Some(source)).expect("play Catalog Preview");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("stable live event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        controller
            .update(|state| state.output_mode = OutputMode::DirectAlsa)
            .expect("select Direct ALSA fixture mode");
        event_sender
            .send(MpvEvent::Error("upstream disconnected".to_owned()))
            .expect("Catalog Preview error");

        let reconnecting = wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Reconnecting
        });
        assert_eq!(reconnecting.status, PlaybackStatus::Reconnecting);
        assert_eq!(reconnecting.error, None);
    }

    #[test]
    fn initial_live_playback_error_remains_terminal() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: Arc::new(AtomicUsize::new(0)),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-initial-error",
                "name": "Initial Error Radio",
                "streamUrl": "https://radio.example/initial-error.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-initial-error/stream",
            "sourceUrl": "https://radio.example/initial-error.mp3"
        });

        controller
            .play(Some(source))
            .expect("start live Radio Station");
        event_sender
            .send(MpvEvent::Error("initial connection failed".to_owned()))
            .expect("initial playback error");

        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);
        assert_eq!(failed.status, PlaybackStatus::Error);
    }

    #[test]
    fn delayed_stable_event_does_not_arm_a_replaced_live_source() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: load_calls.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let first_source = json!({
            "type": "radio-station",
            "station": { "id": "station-first", "name": "First Radio" },
            "playbackUrl": "http://127.0.0.1/api/v1/radio/stations/station-first/stream"
        });
        let replacement_source = json!({
            "type": "radio-station",
            "station": { "id": "station-replacement", "name": "Replacement Radio" },
            "playbackUrl": "http://127.0.0.1/api/v1/radio/stations/station-replacement/stream"
        });

        controller
            .play(Some(first_source))
            .expect("play first live source");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("first source stable event");
        wait_for_state(&controller, |state| state.current_time == 1.0);

        controller
            .play(Some(replacement_source))
            .expect("play replacement live source");
        event_sender
            .send(MpvEvent::Time(2.0))
            .expect("delayed previous-source event");
        wait_for_state(&controller, |state| state.current_time == 2.0);
        event_sender
            .send(MpvEvent::Error("replacement failed initially".to_owned()))
            .expect("replacement initial error");

        let failed = wait_for_state(&controller, |state| state.status == PlaybackStatus::Error);
        thread::sleep(Duration::from_millis(75));
        assert_eq!(failed.status, PlaybackStatus::Error);
        assert_eq!(load_calls.load(Ordering::Acquire), 2);
    }

    #[test]
    fn delayed_stable_event_does_not_complete_a_new_reconnect_load() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: load_calls.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "catalog-preview",
            "result": { "stationUuid": "catalog-delayed", "name": "Delayed Catalog" },
            "playbackUrl": "http://127.0.0.1/api/v1/radio/preview/catalog-delayed/stream"
        });

        controller.play(Some(source)).expect("play Catalog Preview");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("initial stable event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        event_sender
            .send(MpvEvent::Ended)
            .expect("Catalog Preview ended event");
        wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Reconnecting
        });
        let first_retry_deadline = Instant::now() + Duration::from_millis(250);
        while load_calls.load(Ordering::Acquire) < 2 && Instant::now() < first_retry_deadline {
            thread::sleep(Duration::from_millis(5));
        }

        event_sender
            .send(MpvEvent::Time(2.0))
            .expect("delayed pre-reconnect event");
        wait_for_state(&controller, |state| state.current_time == 2.0);
        assert_eq!(
            controller.state().expect("reconnecting state").status,
            PlaybackStatus::Reconnecting
        );
        let readiness_retry_deadline = Instant::now() + Duration::from_millis(350);
        while load_calls.load(Ordering::Acquire) < 3 && Instant::now() < readiness_retry_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(load_calls.load(Ordering::Acquire), 3);
    }

    #[test]
    fn pausing_cancels_a_pending_live_reconnect() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let controller = PlaybackController::start(
            Box::new(ReconnectMpvProcess {
                load_calls: load_calls.clone(),
            }),
            event_receiver,
            |_| {},
        );
        let source = json!({
            "type": "radio-station",
            "station": {
                "id": "station-cancel-reconnect",
                "name": "Cancel Reconnect Radio",
                "streamUrl": "https://radio.example/cancel.mp3"
            },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/radio/stations/station-cancel-reconnect/stream",
            "sourceUrl": "https://radio.example/cancel.mp3"
        });

        controller
            .play(Some(source))
            .expect("play live Radio Station");
        publish_current_load_boundary(&controller, &event_sender);
        event_sender
            .send(MpvEvent::Time(1.0))
            .expect("stable live event");
        wait_for_state(&controller, |state| state.current_time == 1.0);
        event_sender
            .send(MpvEvent::Ended)
            .expect("ended live event");
        wait_for_state(&controller, |state| {
            state.status == PlaybackStatus::Reconnecting
        });

        controller
            .pause()
            .expect("pause reconnecting Radio Station");
        thread::sleep(Duration::from_millis(75));

        assert_eq!(
            controller.state().expect("paused state").status,
            PlaybackStatus::Paused
        );
        assert_eq!(load_calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn native_queue_context_owns_previous_and_next_selection() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let loaded_url = Arc::new(Mutex::new(None));
        let controller = PlaybackController::start(
            Box::new(FakeMpvProcess {
                loaded_url: loaded_url.clone(),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            |_| {},
        );
        let sources = queue_sources();
        controller
            .sync_queue_context(sources.clone(), Some(1))
            .expect("sync Queue context");
        controller
            .play(Some(sources[1].clone()))
            .expect("play middle Track");

        let previous = controller.previous().expect("previous Track");
        let next = controller.next().expect("next Track");

        assert_eq!(previous.source, Some(sources[0].clone()));
        assert_eq!(next.source, Some(sources[1].clone()));
        assert_eq!(
            loaded_url.lock().expect("loaded URL").as_deref(),
            Some("http://127.0.0.1/token/api/v1/tracks/track-2/stream")
        );
    }

    #[test]
    fn native_next_uses_active_source_after_renderer_queue_sync() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            |_| {},
        );
        let sources = queue_sources();
        controller
            .play(Some(sources[0].clone()))
            .expect("restore active Track");
        controller
            .sync_queue_context(sources.clone(), None)
            .expect("renderer attaches Queue");
        assert_eq!(
            controller.next().expect("next Track after reopen").source,
            Some(sources[1].clone())
        );
        controller
            .sync_queue_context(sources.clone(), Some(0))
            .expect("renderer sends stale index");
        assert_eq!(
            controller
                .next()
                .expect("next Track after stale sync")
                .source,
            Some(sources[2].clone())
        );
    }

    struct RecordingMpvProcess {
        actions: Arc<Mutex<Vec<String>>>,
        should_fail_load: Arc<AtomicBool>,
    }

    impl RecordingMpvProcess {
        fn record(&self, action: String) {
            self.actions.lock().expect("recorded actions").push(action);
        }
    }

    impl MpvProcessAdapter for RecordingMpvProcess {
        fn load(&self, url: &str) -> Result<(), String> {
            self.record(format!("load:{url}"));
            if self.should_fail_load.load(Ordering::Acquire) {
                return Err("controlled load failure".to_owned());
            }
            Ok(())
        }

        fn set_paused(&self, is_paused: bool) -> Result<(), String> {
            self.record(format!("pause:{is_paused}"));
            Ok(())
        }

        fn seek(&self, seconds: f64) -> Result<(), String> {
            self.record(format!("seek:{seconds}"));
            Ok(())
        }

        fn set_volume(&self, value: f64) -> Result<(), String> {
            self.record(format!("volume:{value:.2}"));
            Ok(())
        }

        fn set_speed(&self, rate: f64) -> Result<(), String> {
            self.record(format!("speed:{rate}"));
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            self.record("stop".to_owned());
            Ok(())
        }

        fn shutdown(&self) {}
    }

    fn start_recording_controller() -> (PlaybackController, Arc<Mutex<Vec<String>>>) {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let controller = PlaybackController::start(
            Box::new(RecordingMpvProcess {
                actions: actions.clone(),
                should_fail_load: Arc::new(AtomicBool::new(false)),
            }),
            event_receiver,
            |_| {},
        );
        (controller, actions)
    }

    #[test]
    fn playback_rate_is_sent_to_mpv_and_validated() {
        let (controller, actions) = start_recording_controller();

        let state = controller.set_playback_rate(1.5).expect("set speed");
        assert_eq!(state.playback_rate, 1.5);
        assert!(
            actions
                .lock()
                .expect("actions")
                .contains(&"speed:1.5".to_owned())
        );

        let error = controller
            .set_playback_rate(3.0)
            .expect_err("reject an out-of-range speed");
        assert!(error.message.contains("between"));
        assert_eq!(controller.state().expect("state").playback_rate, 1.5);
    }

    #[test]
    fn desktop_rate_action_updates_mpv_and_playback_state() {
        let (controller, actions) = start_recording_controller();
        dispatch_desktop_playback_action(
            DesktopPlaybackAction::SetRate(1.25),
            &controller,
            &Arc::new(Mutex::new(PlaybackLifecycle::new())),
            &PlaybackSnapshotStore::new(temporary_path("rate-action-snapshot.json")),
            &TestDesktopPlaybackShell::default(),
        )
        .expect("dispatch playback rate");

        assert_eq!(controller.state().expect("state").playback_rate, 1.25);
        assert_eq!(*actions.lock().expect("actions"), vec!["speed:1.25"]);
    }

    #[test]
    fn stop_after_current_ends_playback_instead_of_advancing_and_resets() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            |_| {},
        );
        let sources = queue_sources();
        controller
            .sync_queue_context(sources.clone(), Some(0))
            .expect("sync Queue context");
        controller
            .play(Some(sources[0].clone()))
            .expect("play first Track");
        controller
            .set_stop_after_current(true)
            .expect("arm sleep timer");

        event_sender.send(MpvEvent::Ended).expect("ended event");
        let ended = wait_for_state(&controller, |state| state.status == PlaybackStatus::Ended);

        assert_eq!(ended.source, Some(sources[0].clone()));
        assert!(!ended.stop_after_current, "the flag is one-shot");
    }

    #[test]
    fn transition_fade_ramps_volume_around_pause_and_restores_it() {
        let (controller, actions) = start_recording_controller();
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });
        controller.play(Some(source)).expect("play Track");
        controller.set_transition_fade_ms(60).expect("enable fade");
        assert_eq!(controller.transition_fade_ms(), 60);
        actions.lock().expect("actions").clear();

        controller.pause().expect("pause with fade");

        let recorded = actions.lock().expect("actions").clone();
        let pause_index = recorded
            .iter()
            .position(|action| action == "pause:true")
            .expect("pause command recorded");
        let ramp: Vec<&String> = recorded[..pause_index]
            .iter()
            .filter(|action| action.starts_with("volume:"))
            .collect();
        assert_eq!(ramp.len(), 3, "60ms fade in 20ms steps: {recorded:?}");
        assert_eq!(
            ramp.last().map(|action| action.as_str()),
            Some("volume:0.00")
        );
        assert_eq!(
            recorded.get(pause_index + 1).map(String::as_str),
            Some("volume:0.80"),
            "the level is restored after pausing: {recorded:?}"
        );
    }

    #[test]
    fn failed_faded_navigation_restores_volume_before_later_playback() {
        for direction in [super::QueueDirection::Next, super::QueueDirection::Previous] {
            let (_event_sender, event_receiver) = std::sync::mpsc::channel();
            let actions = Arc::new(Mutex::new(Vec::new()));
            let should_fail_load = Arc::new(AtomicBool::new(false));
            let controller = PlaybackController::start(
                Box::new(RecordingMpvProcess {
                    actions: actions.clone(),
                    should_fail_load: should_fail_load.clone(),
                }),
                event_receiver,
                |_| {},
            );
            let sources = queue_sources();
            controller
                .sync_queue_context(sources.clone(), Some(1))
                .expect("sync Queue");
            controller
                .play(Some(sources[1].clone()))
                .expect("play middle Track");
            controller.set_transition_fade_ms(20).expect("enable fade");
            actions.lock().expect("actions").clear();
            should_fail_load.store(true, Ordering::Release);

            controller
                .navigate(direction)
                .expect_err("navigation load fails");
            assert_eq!(
                controller.state().expect("state").status,
                PlaybackStatus::Error
            );
            let recorded = actions.lock().expect("actions").clone();
            assert_eq!(recorded.first().map(String::as_str), Some("volume:0.00"));
            assert_eq!(recorded.last().map(String::as_str), Some("volume:0.80"));

            should_fail_load.store(false, Ordering::Release);
            controller
                .play(Some(sources[1].clone()))
                .expect("retry playback");
            assert_eq!(
                controller.state().expect("state").status,
                PlaybackStatus::Playing
            );
            assert_eq!(
                actions
                    .lock()
                    .expect("actions")
                    .iter()
                    .rev()
                    .find(|action| action.starts_with("volume:"))
                    .map(String::as_str),
                Some("volume:0.80")
            );
        }
    }

    #[test]
    fn transition_fade_is_skipped_when_disabled() {
        let (controller, actions) = start_recording_controller();
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });
        controller.play(Some(source)).expect("play Track");
        actions.lock().expect("actions").clear();

        controller.pause().expect("pause without fade");

        let recorded = actions.lock().expect("actions").clone();
        assert_eq!(recorded, vec!["pause:true".to_owned()]);
    }

    #[test]
    fn native_shuffle_and_end_advancement_use_the_synced_queue_without_mutating_it() {
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        let controller = PlaybackController::start(
            Box::new(FakeMpvProcess {
                loaded_url: Arc::new(Mutex::new(None)),
                is_shutdown: Arc::new(AtomicBool::new(false)),
                load_error: None,
            }),
            event_receiver,
            |_| {},
        );
        let sources = queue_sources();
        controller
            .sync_queue_context(sources.clone(), Some(0))
            .expect("sync Queue context");
        controller
            .play(Some(sources[0].clone()))
            .expect("play first Track");
        controller.toggle_shuffle().expect("enable shuffle");

        event_sender.send(MpvEvent::Ended).expect("ended event");
        let advanced = wait_for_state(&controller, |state| {
            state.source.as_ref() != Some(&sources[0])
        });

        assert_ne!(advanced.source, Some(sources[1].clone()));
        assert!(sources.contains(advanced.source.as_ref().expect("advanced source")));
        assert_eq!(controller.queue_sources().expect("Queue sources"), sources);
    }

    #[test]
    fn process_failures_are_published_as_playback_errors() {
        let (_event_sender, event_receiver) = std::sync::mpsc::channel();
        let process = FakeMpvProcess {
            loaded_url: Arc::new(Mutex::new(None)),
            is_shutdown: Arc::new(AtomicBool::new(false)),
            load_error: Some("fixture load failed".to_owned()),
        };
        let controller = PlaybackController::start(Box::new(process), event_receiver, |_| {});
        let source = json!({
            "type": "track",
            "track": { "id": "track-1", "title": "Track 1", "durationMs": 120000 },
            "playbackUrl": "http://127.0.0.1:43129/token/api/v1/tracks/track-1/stream"
        });

        let error = controller.play(Some(source)).expect_err("load must fail");
        let state = controller.state().expect("playback state");

        assert_eq!(
            error.message,
            "Playback failed. Check the source and try again."
        );
        assert_eq!(state.status, PlaybackStatus::Error);
        assert_eq!(
            state.error.as_ref().map(|error| error.message.as_str()),
            Some("Playback failed. Check the source and try again.")
        );
    }

    fn wait_for_state(
        controller: &PlaybackController,
        predicate: impl Fn(&super::PlaybackSessionState) -> bool,
    ) -> super::PlaybackSessionState {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let state = controller.state().expect("playback state");
            if predicate(&state) {
                return state;
            }
            assert!(Instant::now() < deadline, "timed out waiting for state");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn queue_sources() -> Vec<serde_json::Value> {
        (1..=3)
            .map(|index| {
                json!({
                    "type": "track",
                    "track": {
                        "id": format!("track-{index}"),
                        "title": format!("Track {index}"),
                        "durationMs": 120000
                    },
                    "playbackUrl": format!(
                        "http://127.0.0.1/token/api/v1/tracks/track-{index}/stream"
                    )
                })
            })
            .collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires the pinned Linux mpv executable"]
    fn real_pinned_mpv_controls_a_track_and_removes_private_ipc() {
        let fixture_path = temporary_path("controlled-track.wav");
        fs::write(&fixture_path, silent_wav()).expect("write WAV fixture");
        let (process, events, ipc_directory) =
            RealMpvProcess::start(super::resolve_mpv_binary(), vec!["--ao=null".to_owned()])
                .expect("start pinned mpv");
        assert!(ipc_directory.exists());
        assert_eq!(
            fs::metadata(&ipc_directory)
                .expect("private IPC directory")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(ipc_directory.join("control.sock"))
                .expect("private IPC socket")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let controller = PlaybackController::start(process, events, |_| {});
        let mut processed = ProcessingState {
            profile: crate::processing::ProcessingProfile::Processed,
            software_volume: 0.37,
            replay_gain_mode: ReplayGainMode::Album,
            ..ProcessingState::default()
        };
        processed.equalizer.is_enabled = true;
        let applied = controller
            .apply_processing(
                processed,
                &MpvProcessingConfiguration {
                    volume_percent: 37.0,
                    replay_gain_mode: ReplayGainMode::Album,
                    audio_filters: vec!["equalizer=f=1000:t=q:w=1:g=2".to_owned()],
                },
            )
            .expect("observe effective mpv processing");
        let source = json!({
            "type": "track",
            "track": { "id": "fixture", "title": "Fixture", "durationMs": 1000 },
            "playbackUrl": fixture_path.to_string_lossy()
        });

        controller.play(Some(source.clone())).expect("play fixture");
        let playing = wait_for_state(&controller, |state| state.current_time > 0.0);
        controller.pause().expect("pause fixture");
        controller.seek(0.5).expect("seek fixture");
        let snapshot = PlaybackSessionSnapshot::new(Some(source), 0.5, 0.37, false, "off")
            .expect("valid real mpv snapshot");
        let restored = controller
            .restore_paused(&snapshot)
            .expect("restore real mpv snapshot without an early seek command");

        assert_eq!(applied.processing.software_volume, 0.37);
        assert_eq!(applied.processing.replay_gain_mode, ReplayGainMode::Album);
        assert!(applied.processing.equalizer.is_enabled);
        assert!(!applied.processing.effective_audio_filters.is_empty());
        assert_eq!(playing.status, PlaybackStatus::Playing);
        assert!(playing.duration > 0.9);
        assert_eq!(restored.status, PlaybackStatus::Paused);
        assert_eq!(restored.current_time, 0.5);
        drop(controller);
        assert!(!ipc_directory.exists());
        fs::remove_file(fixture_path).expect("remove WAV fixture");
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "temporarily disables the active PipeWire card profile and uses ALSA card 2"]
    fn real_pinned_mpv_returns_from_exclusive_to_system_output() {
        let fixture_path = temporary_path("exclusive-transition.wav");
        fs::write(&fixture_path, silent_48khz_stereo_wav()).expect("write WAV fixture");
        let (process, _events, _ipc_directory) = RealMpvProcess::start(
            super::resolve_mpv_binary(),
            vec!["--loop-file=inf".to_owned()],
        )
        .expect("start pinned mpv");
        process
            .load(&fixture_path.to_string_lossy())
            .expect("load transition fixture");
        process.set_paused(false).expect("play transition fixture");
        wait_for_real_output(process.as_ref(), "initial System Output");

        for transition in 1..=2 {
            process
                .configure_output(&MpvOutputConfiguration::DirectAlsa {
                    device_id: "hw:2,0".to_owned(),
                })
                .expect("open Exclusive Output");
            wait_for_real_output(
                process.as_ref(),
                &format!("Exclusive Output transition {transition}"),
            );
            process
                .configure_output(&MpvOutputConfiguration::System)
                .expect("restore System Output");
            wait_for_real_output(
                process.as_ref(),
                &format!("System Output transition {transition}"),
            );
        }

        drop(process);
        fs::remove_file(fixture_path).expect("remove WAV fixture");
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires the pinned Linux mpv executable and desktop audio lifecycle"]
    fn linux_background_playback_continues_until_clean_explicit_quit() {
        let fixture_path = temporary_path("background-track.wav");
        let snapshot_path = temporary_path("playback-session.json");
        fs::write(&fixture_path, silent_wav()).expect("write WAV fixture");
        let (process, events, ipc_directory) =
            RealMpvProcess::start(super::resolve_mpv_binary(), vec!["--ao=null".to_owned()])
                .expect("start pinned mpv");
        let controller = PlaybackController::start(process, events, |_| {});
        let source = json!({
            "type": "track",
            "track": { "id": "fixture", "title": "Fixture", "durationMs": 1000 },
            "playbackUrl": fixture_path.to_string_lossy()
        });
        controller.play(Some(source)).expect("play fixture");
        let before_close = wait_for_state(&controller, |state| state.current_time > 0.0);
        let lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
        let snapshot_store = PlaybackSnapshotStore::new(snapshot_path.clone());
        let shell = TestDesktopPlaybackShell::default();

        dispatch_desktop_playback_action(
            DesktopPlaybackAction::CloseMainWindow,
            &controller,
            &lifecycle,
            &snapshot_store,
            &shell,
        )
        .expect("dispatch actual close action");
        let background = wait_for_state(&controller, |state| {
            state.current_time > before_close.current_time
        });
        dispatch_desktop_playback_action(
            DesktopPlaybackAction::Quit,
            &controller,
            &lifecycle,
            &snapshot_store,
            &shell,
        )
        .expect("dispatch actual tray Quit action");
        drop(controller);

        assert!(shell.is_hidden.load(Ordering::Acquire));
        assert!(shell.has_exited.load(Ordering::Acquire));
        assert!(background.current_time > before_close.current_time);
        assert!(snapshot_path.exists());
        assert!(!ipc_directory.exists());
        fs::remove_file(fixture_path).expect("remove WAV fixture");
        fs::remove_file(snapshot_path).expect("remove snapshot fixture");
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires pinned Linux mpv and ffmpeg fixture generation"]
    fn real_pinned_mpv_streams_controlled_mp3_icy_and_aac_hls_sources() {
        let fixture_directory = temporary_path("controlled-streams");
        fs::create_dir(&fixture_directory).expect("create stream fixture directory");
        let mp3 = generate_audio_fixture(&fixture_directory, "radio.mp3", "libmp3lame");
        let aac = generate_audio_fixture(&fixture_directory, "segment.aac", "aac");
        let server = ControlledStreamServer::start(mp3, aac);
        let (process, events, _) =
            RealMpvProcess::start(super::resolve_mpv_binary(), vec!["--ao=null".to_owned()])
                .expect("start pinned mpv");
        let controller = PlaybackController::start(process, events, |_| {});

        play_controlled_stream(
            &controller,
            "radio-station",
            format!("{}/radio.mp3", server.base_url),
        );
        play_controlled_stream(
            &controller,
            "catalog-preview",
            format!("{}/catalog.m3u8", server.base_url),
        );

        drop(controller);
        server.shutdown();
        fs::remove_dir_all(fixture_directory).expect("remove stream fixture directory");
    }

    fn play_controlled_stream(
        controller: &PlaybackController,
        source_type: &str,
        playback_url: String,
    ) {
        let source = match source_type {
            "radio-station" => json!({
                "type": source_type,
                "station": { "id": "controlled-radio", "name": "Controlled ICY MP3" },
                "playbackUrl": playback_url
            }),
            _ => json!({
                "type": source_type,
                "result": { "stationUuid": "controlled-catalog", "name": "Controlled AAC HLS" },
                "playbackUrl": playback_url
            }),
        };
        controller
            .play(Some(source))
            .expect("play controlled stream");
        wait_for_state(controller, |state| state.current_time > 0.05);
    }

    #[test]
    #[ignore = "requires pinned mpv"]
    fn real_pinned_mpv_exits_when_desktop_lifetime_connection_closes() {
        let directory = super::create_private_ipc_directory().expect("private IPC directory");
        let socket_path = directory.join("control.sock");
        let (lifetime_stream, child_stream) =
            std::os::unix::net::UnixStream::pair().expect("lifetime socket pair");
        let mut command = Command::new(super::resolve_mpv_binary());
        super::configure_mpv_command(&mut command, &socket_path, vec!["--ao=null".to_owned()]);
        command.stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
            child_stream,
        )));
        let mut child = command.spawn().expect("spawn pinned mpv");
        drop(command);
        super::wait_for_mpv_socket(&mut child, &socket_path).expect("mpv ready");
        assert!(child.try_wait().expect("running child status").is_none());
        drop(lifetime_stream);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut has_exited = false;
        while Instant::now() < deadline {
            if child.try_wait().expect("child status").is_some() {
                has_exited = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        super::terminate_mpv(&mut child);
        fs::remove_dir_all(directory).expect("remove private IPC directory");
        assert!(
            has_exited,
            "mpv survived the Desktop Client lifetime connection"
        );
    }

    #[test]
    fn mpv_uses_only_application_owned_audio_configuration() {
        let mut command = Command::new("mpv");
        let socket_path = PathBuf::from("/private/control.sock");

        super::configure_mpv_command(&mut command, &socket_path, Vec::new());

        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"--no-config".to_owned()));
        assert!(arguments.contains(&"--idle=yes".to_owned()));
        assert!(arguments.contains(&"--no-video".to_owned()));
        assert!(arguments.contains(&"--input-terminal=no".to_owned()));
        assert!(arguments.contains(&"--gapless-audio=weak".to_owned()));
        assert!(arguments.contains(&"--audio-pitch-correction=yes".to_owned()));
        assert!(arguments.contains(&"--input-ipc-server=/private/control.sock".to_owned()));
        assert!(!arguments.iter().any(|argument| {
            argument.contains("audio-exclusive") || argument.contains("exclusive")
        }));
    }

    #[test]
    fn desktop_config_packages_the_pinned_mpv_sidecar() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.sidecar.conf.json"))
                .expect("Tauri sidecar config");

        assert_eq!(
            config.pointer("/bundle/externalBin"),
            Some(&json!(["binaries/mpv"]))
        );
    }

    fn temporary_path(file_name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).expect("random temporary name");
        std::env::temp_dir().join(format!(
            "earthly-audio-test-{}-{file_name}",
            u64::from_le_bytes(random)
        ))
    }

    fn silent_wav() -> Vec<u8> {
        const SAMPLE_RATE: u32 = 8_000;
        const DATA_SIZE: u32 = SAMPLE_RATE * 2;
        let mut wav = Vec::with_capacity(44 + DATA_SIZE as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + DATA_SIZE).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&DATA_SIZE.to_le_bytes());
        wav.resize(44 + DATA_SIZE as usize, 0);
        wav
    }

    fn silent_48khz_stereo_wav() -> Vec<u8> {
        const SAMPLE_RATE: u32 = 48_000;
        const CHANNELS: u16 = 2;
        const BYTES_PER_SAMPLE: u16 = 2;
        const DATA_SIZE: u32 = SAMPLE_RATE * CHANNELS as u32 * BYTES_PER_SAMPLE as u32;
        let block_alignment = CHANNELS * BYTES_PER_SAMPLE;
        let mut wav = Vec::with_capacity(44 + DATA_SIZE as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + DATA_SIZE).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0");
        wav.extend_from_slice(&CHANNELS.to_le_bytes());
        wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        wav.extend_from_slice(&(SAMPLE_RATE * block_alignment as u32).to_le_bytes());
        wav.extend_from_slice(&block_alignment.to_le_bytes());
        wav.extend_from_slice(&(BYTES_PER_SAMPLE * 8).to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&DATA_SIZE.to_le_bytes());
        wav.resize(44 + DATA_SIZE as usize, 0);
        wav
    }

    fn wait_for_real_output(
        process: &dyn MpvProcessAdapter,
        context: &str,
    ) -> AudioFormatObservation {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(Some(format)) = process.observe_output_format() {
                return format;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {context}");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
