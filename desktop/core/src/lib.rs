pub mod adaptive_system_rate;
pub mod connection;
pub mod desktop_import;
mod device_volume;
mod exclusive_output;
pub mod media_proxy;
#[cfg(target_os = "linux")]
mod mpris;
#[cfg(not(target_os = "linux"))]
#[path = "mpris_unsupported.rs"]
mod mpris;
#[cfg(windows)]
mod mpv_pipe;
pub mod output_device;
mod playback;
mod playback_app_actions;
mod playback_lifecycle;
mod playback_navigation;
mod playback_publish;
mod playback_restore;
#[cfg(test)]
// Its stream fixtures serve the Linux-only real-mpv tests.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod playback_test_support;
mod playback_tray;
#[cfg(windows)]
mod policy_config;
pub mod processing;
mod queue_events;
mod rpc;
pub mod runtime;
pub mod telemetry;
#[cfg(windows)]
mod wasapi;

use adaptive_system_rate::{
    AdaptiveSystemRateController, FileAdaptiveCleanupMarker, platform_rate_adapter,
};
use connection::{
    ConnectionCheck, ConnectionError, ConnectionErrorCode, ConnectionStore, HttpBridge,
    HttpRequest, HttpResponse, ServerOrigin,
};
use desktop_import::{
    DesktopImportError, ImportSelection, ImportSelectionStore, ImportUploadProgress, ProgressSink,
    upload_selection,
};
use device_volume::{DeviceVolume, PlatformDeviceVolume, VolumeRoute, hardware_step_db};
use media_proxy::MediaProxy;
use mpris::MprisBridge;
use output_device::{
    ActiveOutputError, ActiveOutputResolver, OutputDevice, PlatformActiveOutputResolver,
};
use playback::{
    OutputDeviceIssueCode, PlaybackCommandError, PlaybackController, PlaybackSessionState,
};
use playback_app_actions::{
    DesktopPlaybackAction, DesktopPlaybackShell, dispatch_desktop_playback_action,
};
use playback_lifecycle::{PlaybackLifecycle, PlaybackSnapshotStore};
use playback_publish::PlayheadThrottle;
use playback_tray::{PlaybackPosition, PlaybackTray, PlaybackTrayView};
use processing::{
    EqualizerPreset, FileProcessingSettingsStorage, OutputMode, ProcessingController,
    ProcessingProfile, ReplayGainMode, mpv_configuration_for,
};
use queue_events::QueueEventService;
use rpc::{Output, Reply, Request, param, reply};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak, mpsc};
use std::time::Duration;
use telemetry::PlatformPathObserver;

const CONNECTION_FILE_NAME: &str = "server-connection.json";
const PLAYBACK_SNAPSHOT_FILE_NAME: &str = "playback-session.json";
const PROCESSING_SETTINGS_FILE_NAME: &str = "processing-settings.json";
const ADAPTIVE_CLEANUP_MARKER_FILE_NAME: &str = "adaptive-system-rate.cleanup-required";
const DEVICE_ATTENUATION_FILE_NAME: &str = "device-volume-hand-off.json";
const PLAYBACK_STATE_EVENT: &str = "desktop-playback-state";
const CONNECTION_CHANGED_EVENT: &str = "server-connection-changed";
const QUEUE_EVENTS_ERROR_EVENT: &str = "desktop-queue-events-error";
const QUEUE_INVALIDATED_EVENT: &str = "desktop-queue-invalidated";
const IMPORT_PROGRESS_EVENT: &str = "desktop-import-progress";
const SHELL_SHOW_MAIN_WINDOW: &str = "shell:show-main-window";
const SHELL_HIDE_MAIN_WINDOW: &str = "shell:hide-main-window";
const SHELL_TOGGLE_MINI_WINDOW: &str = "shell:toggle-mini-window";
const SHELL_WINDOW_TITLE: &str = "shell:window-title";
const SHELL_TRAY: &str = "shell:tray";
const SHELL_EXIT: &str = "shell:exit";
const SHELL_READY: &str = "shell:ready";

struct AppState {
    playback: PlaybackController,
    processing: Mutex<ProcessingController>,
    active_output_resolver: Arc<dyn ActiveOutputResolver>,
    playback_lifecycle: Arc<Mutex<PlaybackLifecycle>>,
    playback_snapshot_store: PlaybackSnapshotStore,
    bridge: Arc<HttpBridge>,
    store: ConnectionStore,
    origin: Arc<RwLock<Option<ServerOrigin>>>,
    media_proxy: MediaProxy,
    queue_events: QueueEventService,
    import_selections: ImportSelectionStore,
    output: Arc<Output>,
    /// Set once the core has asked the shell to exit, so closing stdin after
    /// an explicit Quit does not quit (and snapshot) a second time.
    exiting: AtomicBool,
    exit: mpsc::Sender<()>,
    device_volume: Arc<dyn DeviceVolume>,
    device_attenuation: Mutex<Option<DeviceAttenuation>>,
    /// Where a hand-off is recorded until it is undone.
    device_attenuation_record: PathBuf,
}

/// Attenuation moved from mpv onto the output device while a bit-perfect
/// Output Mode is active, and the software volume it replaced.
struct DeviceAttenuation {
    route: VolumeRoute,
    applied_db: f64,
    software_volume: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerConnection {
    origin: String,
}

fn get_server_connection(state: &AppState) -> Result<Option<ServerConnection>, ConnectionError> {
    let origin = state.origin.read().map_err(|_| state_error())?;
    Ok(origin.as_ref().map(|origin| ServerConnection {
        origin: origin.as_str().to_owned(),
    }))
}

async fn test_server_connection(
    state: &AppState,
    origin: String,
) -> Result<ConnectionCheck, ConnectionError> {
    let origin = ServerOrigin::parse(&origin)?;
    state.bridge.test_server(&origin).await
}

async fn save_server_connection(
    state: &AppState,
    origin: String,
) -> Result<ConnectionCheck, ConnectionError> {
    let origin = ServerOrigin::parse(&origin)?;
    let check = state.bridge.test_server(&origin).await?;
    state.store.save(&origin)?;
    *state.origin.write().map_err(|_| state_error())? = Some(origin);
    state.queue_events.reconnect();
    state.output.emit(CONNECTION_CHANGED_EVENT, &check);
    Ok(check)
}

async fn desktop_http_request(
    state: &AppState,
    request: HttpRequest,
) -> Result<HttpResponse, ConnectionError> {
    let origin = state
        .origin
        .read()
        .map_err(|_| state_error())?
        .clone()
        .ok_or_else(|| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Configure a Music Server before sending desktop HTTP requests.",
            )
        })?;
    state.bridge.send(&origin, request).await
}

/// Registers what the shell's native picker returned. Only the shell calls
/// this: the renderer never hands the core a path, it receives opaque
/// selection IDs.
fn desktop_register_import_paths(
    state: &AppState,
    paths: Vec<PathBuf>,
) -> Result<Vec<ImportSelection>, DesktopImportError> {
    state.import_selections.register_paths(paths)
}

async fn desktop_upload_import_file(
    state: &AppState,
    selection_id: String,
    upload_id: String,
    job_id: String,
) -> Result<HttpResponse, DesktopImportError> {
    let origin = state
        .origin
        .read()
        .map_err(|_| desktop_import_state_error())?
        .clone()
        .ok_or_else(|| DesktopImportError {
            code: "invalid_origin",
            message: "Configure a Music Server before uploading music.".to_owned(),
        })?;
    let output = state.output.clone();
    let progress_upload_id = upload_id.clone();
    let on_progress: ProgressSink = Arc::new(move |progress: ImportUploadProgress| {
        output.emit(
            IMPORT_PROGRESS_EVENT,
            json!({
                "uploadId": progress_upload_id,
                "sentBytes": progress.sent_bytes,
                "totalBytes": progress.total_bytes,
            }),
        );
    });
    upload_selection(
        &state.import_selections,
        &state.bridge,
        &origin,
        &selection_id,
        &upload_id,
        &job_id,
        on_progress,
    )
    .await
}

fn desktop_import_state_error() -> DesktopImportError {
    DesktopImportError {
        code: "state_unavailable",
        message: "Desktop import state is unavailable.".to_owned(),
    }
}

fn desktop_playback_renderer_ready(
    state: &AppState,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state
        .playback_lifecycle
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback lifecycle state is unavailable."))?
        .renderer_attached();
    let playback_state = state.playback.state()?;
    state.output.emit(PLAYBACK_STATE_EVENT, &playback_state);
    Ok(playback_state)
}

fn desktop_playback_play(
    state: &AppState,
    source: Option<Value>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    if let Some(source) = source.as_ref() {
        validate_playback_source(source, state.media_proxy.base_url())?;
    }
    state.playback.play(source)
}

fn desktop_playback_sync_queue_context(
    state: &AppState,
    sources: Vec<Value>,
    current_index: Option<usize>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    for source in &sources {
        validate_playback_source(source, state.media_proxy.base_url())?;
    }
    state.playback.sync_queue_context(sources, current_index)
}

fn desktop_playback_set_replay_gain(
    state: &AppState,
    mode: ReplayGainMode,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(state, |processing| {
        if mode == ReplayGainMode::Off {
            processing.disable_replay_gain()
        } else {
            processing.enable_replay_gain(mode)
        }
    })
}

fn desktop_playback_select_direct_alsa_output(
    state: &AppState,
    device_id: String,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
    return_device_volume(state, &mut processing);
    let previous_mode = processing.output_mode();
    let previous_device_id = processing.selected_output_device_id().map(str::to_owned);
    let playback_state = state.playback.select_direct_alsa_output(&device_id)?;
    if !is_valid_direct_selection(&playback_state, &device_id) {
        return Ok(playback_state);
    }
    if let Err(error) = processing.select_direct_alsa_output(&device_id) {
        eprintln!("Direct ALSA Output preference persistence failed: {error}");
        restore_native_output_mode(
            &state.playback,
            previous_mode,
            previous_device_id.as_deref(),
        );
        return Err(PlaybackCommandError::new(
            "Direct ALSA Output preference could not be saved. The previous Output Mode was restored.",
        ));
    }
    Ok(restore_remembered_volume(
        state,
        &mut processing,
        playback_state,
    ))
}

fn desktop_playback_select_exclusive_output(
    state: &AppState,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let device = match state.active_output_resolver.resolve() {
        Ok(device) => device,
        Err(error) => {
            select_normal_output(state)?;
            return Err(PlaybackCommandError::new(active_output_error_message(
                &error,
            )));
        }
    };
    select_exclusive_output(state, device)
}

fn desktop_playback_enable_adaptive_system_rate(
    state: &AppState,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
    return_device_volume(state, &mut processing);
    let playback_state = state.playback.enable_adaptive_system_rate()?;
    if let Err(error) = processing.set_output_mode(OutputMode::AdaptiveSystemRate) {
        eprintln!("Adaptive System Rate preference persistence failed: {error}");
        if let Err(rollback_error) = state.playback.fallback_to_system_output() {
            eprintln!(
                "Adaptive System Rate rollback failed after persistence error: {}",
                rollback_error.message
            );
        }
        return Err(PlaybackCommandError::new(
            "Adaptive System Rate preference could not be saved. System Output was restored.",
        ));
    }
    Ok(hand_off_volume_to_device(
        state,
        &mut processing,
        VolumeRoute::SystemSink,
        playback_state,
    ))
}

fn is_valid_direct_selection(state: &PlaybackSessionState, device_id: &str) -> bool {
    let is_selected = state
        .selected_output_device
        .as_ref()
        .is_some_and(|device| device.id == device_id);
    let is_busy_or_unsupported = state
        .output_device_issue
        .as_ref()
        .is_some_and(|issue| issue.code == OutputDeviceIssueCode::BusyOrUnsupported);
    is_selected && (state.output_mode == OutputMode::DirectAlsa || is_busy_or_unsupported)
}

fn restore_native_output_mode(
    playback: &PlaybackController,
    output_mode: OutputMode,
    device_id: Option<&str>,
) {
    let result = match (output_mode, device_id) {
        (OutputMode::DirectAlsa, Some(device_id)) => {
            playback.select_direct_alsa_output(device_id).map(|_| ())
        }
        (OutputMode::AdaptiveSystemRate, _) => playback.enable_adaptive_system_rate().map(|_| ()),
        _ => playback.fallback_to_system_output().map(|_| ()),
    };
    if let Err(error) = result {
        eprintln!("Output Mode rollback failed: {}", error.message);
    }
}

fn select_normal_output(state: &AppState) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
    return_device_volume(state, &mut processing);
    let previous_mode = processing.output_mode();
    let previous_device_id = processing.selected_output_device_id().map(str::to_owned);
    let playback_state = state.playback.fallback_to_system_output()?;
    if let Err(error) = processing.set_output_mode(OutputMode::System) {
        restore_native_output_mode(
            &state.playback,
            previous_mode,
            previous_device_id.as_deref(),
        );
        return Err(PlaybackCommandError::new(format!(
            "Normal Output preference could not be saved: {error}"
        )));
    }
    Ok(restore_remembered_volume(
        state,
        &mut processing,
        playback_state,
    ))
}

fn select_exclusive_output(
    state: &AppState,
    device: OutputDevice,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
    return_device_volume(state, &mut processing);
    let playback_state = state.playback.select_direct_alsa_output(&device.id)?;
    if playback_state.output_mode != OutputMode::DirectAlsa {
        let message = playback_state
            .output_device_issue
            .as_ref()
            .map(|issue| issue.message.clone())
            .unwrap_or_else(|| "Exclusive Output could not open the active device.".to_owned());
        state.playback.fallback_to_system_output()?;
        processing
            .set_output_mode(OutputMode::System)
            .map_err(PlaybackCommandError::new)?;
        return Err(PlaybackCommandError::new(message));
    }
    if let Err(error) = processing.select_direct_alsa_output(&device.id) {
        eprintln!("Exclusive Output preference persistence failed: {error}");
        if let Err(fallback_error) = state.playback.fallback_to_system_output() {
            eprintln!(
                "Normal Output recovery failed after Exclusive persistence error: {}",
                fallback_error.message
            );
        }
        if let Err(persistence_error) = processing.set_output_mode(OutputMode::System) {
            eprintln!("Normal Output recovery persistence failed: {persistence_error}");
        }
        return Err(PlaybackCommandError::new(
            "Exclusive Output preference could not be saved. Normal Output was restored.",
        ));
    }
    let Some(route) = VolumeRoute::for_alsa_device(&device.id) else {
        return Ok(playback_state);
    };
    Ok(hand_off_volume_to_device(
        state,
        &mut processing,
        route,
        playback_state,
    ))
}

fn active_output_error_message(error: &ActiveOutputError) -> String {
    match error {
        ActiveOutputError::NoActiveOutput => {
            "Exclusive Output is unavailable because no active system output was found. Normal Output remains active."
        }
        ActiveOutputError::NotAlsaBacked => {
            "Exclusive Output is unavailable for the active system output. Normal Output remains active."
        }
        ActiveOutputError::QueryFailed(_) => {
            "Exclusive Output could not inspect the active system output. Normal Output remains active."
        }
    }
    .to_owned()
}

fn restore_saved_exclusive_output(
    playback: &PlaybackController,
    processing: &mut ProcessingController,
    resolver: &dyn ActiveOutputResolver,
) {
    let result = resolver
        .resolve()
        .map_err(|error| active_output_error_message(&error))
        .and_then(|device| restore_exclusive_device(playback, processing, &device));
    if let Err(error) = result {
        eprintln!("Saved Exclusive Output could not be restored: {error}");
        if let Err(fallback_error) = playback.fallback_to_system_output() {
            eprintln!(
                "Normal Output startup recovery failed: {}",
                fallback_error.message
            );
        }
        if let Err(persistence_error) = processing.set_output_mode(OutputMode::System) {
            eprintln!("Normal Output startup persistence failed: {persistence_error}");
        }
    }
}

fn restore_exclusive_device(
    playback: &PlaybackController,
    processing: &mut ProcessingController,
    device: &OutputDevice,
) -> Result<(), String> {
    let state = playback
        .select_direct_alsa_output(&device.id)
        .map_err(|error| error.message)?;
    if state.output_mode != OutputMode::DirectAlsa {
        return Err("the active output could not be opened exclusively".to_owned());
    }
    processing.select_direct_alsa_output(&device.id)
}

/// Bit-perfect Output Modes keep mpv at full volume, so the listener's
/// software attenuation moves onto the device first and loudness never jumps.
/// When the device cannot take all of it, mpv keeps its volume and the mode is
/// simply not bit-perfect.
fn hand_off_volume_to_device(
    state: &AppState,
    processing: &mut ProcessingController,
    route: VolumeRoute,
    playback_state: PlaybackSessionState,
) -> PlaybackSessionState {
    let software_volume = processing.state().software_volume;
    if !(software_volume > 0.0 && software_volume < 1.0) {
        return playback_state;
    }
    let wanted_db = hardware_step_db(software_volume);
    let applied_db = match state.device_volume.change_db(route, wanted_db) {
        Ok(applied_db) => applied_db,
        Err(error) => {
            eprintln!("Output volume hand-off failed; mpv keeps its volume: {error}");
            return playback_state;
        }
    };
    if (applied_db - wanted_db).abs() > 0.01 {
        if let Err(error) = state.device_volume.change_db(route, -applied_db) {
            eprintln!("Partial output volume hand-off could not be undone: {error}");
        }
        return playback_state;
    }
    if let Ok(mut attenuation) = state.device_attenuation.lock() {
        *attenuation = Some(DeviceAttenuation {
            route,
            applied_db,
            software_volume,
        });
    }
    save_device_attenuation_record(
        &state.device_attenuation_record,
        route,
        applied_db,
        software_volume,
    );
    let is_volume_only = !processing.state().equalizer.is_enabled
        && processing.state().replay_gain_mode == ReplayGainMode::Off;
    match update_processing_locked(state, processing, |processing| {
        if is_volume_only {
            processing.set_profile(ProcessingProfile::Direct)
        } else {
            processing.set_software_volume(1.0)
        }
    }) {
        Ok(handed_off) => handed_off,
        Err(error) => {
            eprintln!(
                "mpv volume could not follow the output hand-off: {}",
                error.message
            );
            playback_state
        }
    }
}

/// Undoes `hand_off_volume_to_device`: mpv first, then the device, so the
/// level only ever dips while the Output Mode changes. PipeWire restores an
/// ALSA card's own volume when it takes the card back from Exclusive Output.
fn return_device_volume(state: &AppState, processing: &mut ProcessingController) {
    let Some(attenuation) = state
        .device_attenuation
        .lock()
        .ok()
        .and_then(|mut attenuation| attenuation.take())
    else {
        return;
    };
    let volume = returned_volume(
        processing.state().software_volume,
        attenuation.software_volume,
    );
    if let Err(error) = update_processing_locked(state, processing, |processing| {
        processing.set_software_volume(volume)
    }) {
        eprintln!("mpv volume could not be restored: {}", error.message);
        // mpv is already gone at exit; the next start must still open at the
        // listener's level, not at full volume.
        if let Err(error) = processing.set_software_volume(volume) {
            eprintln!("Software volume could not be saved: {error}");
        }
    }
    let is_device_restored = attenuation.route != VolumeRoute::SystemSink
        || match state
            .device_volume
            .change_db(VolumeRoute::SystemSink, -attenuation.applied_db)
        {
            Ok(_) => true,
            Err(error) => {
                eprintln!("System output volume could not be restored: {error}");
                false
            }
        };
    // A failed restore keeps the record, so the next start retries it.
    if is_device_restored {
        clear_device_attenuation_record(&state.device_attenuation_record);
    }
}

/// A hand-off as it stands on the device, so a client killed before
/// `return_device_volume` has it undone at the next start: mpv's volume was
/// saved at full, and a system output left attenuated (the PipeWire sink, the
/// Windows endpoint) would quiet every application and add up across starts.
#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAttenuationRecord {
    route: RecordedRoute,
    /// The sink or endpoint that carries it; none for a raw ALSA card, whose
    /// own volume PipeWire restores when it takes the card back.
    target: Option<String>,
    applied_db: f64,
    software_volume: f64,
}

#[derive(Clone, Copy, serde::Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RecordedRoute {
    SystemSink,
    AlsaCard,
}

#[cfg(not(windows))]
fn hand_off_target(route: VolumeRoute) -> Result<Option<String>, String> {
    match route {
        VolumeRoute::SystemSink => device_volume::default_sink_name().map(Some),
        VolumeRoute::AlsaCard(_) => Ok(None),
    }
}

#[cfg(windows)]
fn hand_off_target(_route: VolumeRoute) -> Result<Option<String>, String> {
    wasapi::default_endpoint_id().map(Some)
}

/// `Ok(false)` while the device that carries it is absent.
#[cfg(not(windows))]
fn undo_recorded_hand_off(record: &DeviceAttenuationRecord) -> Result<bool, String> {
    match (record.route, record.target.as_deref()) {
        (RecordedRoute::SystemSink, Some(sink)) => {
            device_volume::change_named_sink_db(sink, -record.applied_db).map(|_| true)
        }
        _ => Ok(true),
    }
}

#[cfg(windows)]
fn undo_recorded_hand_off(record: &DeviceAttenuationRecord) -> Result<bool, String> {
    match record.target.as_deref() {
        Some(endpoint) => wasapi::change_named_endpoint_db(endpoint, -record.applied_db),
        None => Ok(true),
    }
}

fn save_device_attenuation_record(
    path: &Path,
    route: VolumeRoute,
    applied_db: f64,
    software_volume: f64,
) {
    let target = match hand_off_target(route) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("Output volume hand-off could not be recorded: {error}");
            return;
        }
    };
    let record = DeviceAttenuationRecord {
        route: match route {
            VolumeRoute::SystemSink => RecordedRoute::SystemSink,
            VolumeRoute::AlsaCard(_) => RecordedRoute::AlsaCard,
        },
        target,
        applied_db,
        software_volume,
    };
    let result = serde_json::to_string(&record)
        .map_err(|error| error.to_string())
        .and_then(|text| std::fs::write(path, text).map_err(|error| error.to_string()));
    if let Err(error) = result {
        eprintln!("Output volume hand-off could not be recorded: {error}");
    }
}

fn clear_device_attenuation_record(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("Output volume hand-off record could not be cleared: {error}");
    }
}

/// Undoes a hand-off a killed client left on the device. mpv's volume was
/// saved at full while the device carried the attenuation, so the listener's
/// level is put back first: nothing plays louder than they chose.
fn recover_left_device_attenuation(path: &Path, processing: &mut ProcessingController) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let record: DeviceAttenuationRecord = match serde_json::from_str(&text) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("Output volume hand-off record is unreadable: {error}");
            clear_device_attenuation_record(path);
            return;
        }
    };
    // As `return_device_volume` does: the slider's value since the hand-off
    // on top of the level the device carried, so a change made meanwhile
    // survives and nothing plays louder.
    let volume = returned_volume(processing.state().software_volume, record.software_volume);
    if let Err(error) = processing.set_software_volume(volume) {
        eprintln!("Software volume could not be restored after a hand-off: {error}");
    }
    match undo_recorded_hand_off(&record) {
        Ok(true) => {
            eprintln!("Output volume hand-off left by the previous run was undone.");
            clear_device_attenuation_record(path);
        }
        Ok(false) => {
            eprintln!("Output volume hand-off left on an absent output is kept until it returns.")
        }
        Err(error) => eprintln!("Output volume hand-off could not be undone: {error}"),
    }
}

/// The software volume that keeps the level heard while the device carried
/// `entry`: the slider's current value on top of the device's attenuation,
/// so a change made during the hand-off survives it (and nothing jumps).
fn returned_volume(current: f64, entry: f64) -> f64 {
    current * entry
}

/// After an output route switch, bring back the volume last used on that
/// route. Failures only log: the switch itself already succeeded.
fn restore_remembered_volume(
    state: &AppState,
    processing: &mut ProcessingController,
    playback_state: PlaybackSessionState,
) -> PlaybackSessionState {
    let Some(volume) = processing.remembered_volume() else {
        return playback_state;
    };
    if (volume - processing.state().software_volume).abs() < f64::EPSILON {
        return playback_state;
    }
    match update_processing_locked(state, processing, |processing| {
        processing.set_software_volume(volume)
    }) {
        Ok(restored) => restored,
        Err(error) => {
            eprintln!(
                "Remembered output volume could not be restored: {}",
                error.message
            );
            playback_state
        }
    }
}

fn update_processing(
    state: &AppState,
    change: impl FnOnce(&mut ProcessingController) -> Result<(), String>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Processing Profile state is unavailable."))?;
    update_processing_locked(state, &mut processing, change)
}

fn update_processing_locked(
    state: &AppState,
    processing: &mut ProcessingController,
    change: impl FnOnce(&mut ProcessingController) -> Result<(), String>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let previous_state = processing.snapshot();
    change(processing).map_err(PlaybackCommandError::new)?;
    let processing_state = processing.state().clone();
    let configuration = processing.mpv_configuration();
    match state
        .playback
        .apply_processing(processing_state, &configuration)
    {
        Ok(playback_state) => {
            if let Err(error) = processing.restore(playback_state.processing.clone()) {
                rollback_processing(&state.playback, processing, previous_state);
                return Err(PlaybackCommandError::new(error));
            }
            Ok(playback_state)
        }
        Err(error) => {
            rollback_processing(&state.playback, processing, previous_state);
            Err(error)
        }
    }
}

fn rollback_processing(
    playback: &PlaybackController,
    processing: &mut ProcessingController,
    previous_state: processing::ProcessingSnapshot,
) {
    let configuration = mpv_configuration_for(&previous_state.state);
    let processing_state = previous_state.state.clone();
    if let Err(error) = processing.restore_snapshot(previous_state) {
        eprintln!("Processing Profile rollback persistence failed: {error}");
    }
    if let Err(error) = playback.apply_processing(processing_state, &configuration) {
        eprintln!(
            "Native mpv Processing Profile rollback failed: {}",
            error.message
        );
    }
}

fn validate_playback_source(
    source: &Value,
    media_proxy_base_url: &str,
) -> Result<(), PlaybackCommandError> {
    let playback_url = source
        .get("playbackUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| PlaybackCommandError::new("Playback Source URL is missing."))?;
    let required_prefix = format!("{media_proxy_base_url}/");
    if !playback_url.starts_with(&required_prefix) {
        return Err(PlaybackCommandError::new(
            "Native playback is restricted to the private Desktop media proxy.",
        ));
    }
    Ok(())
}

fn state_error() -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorCode::Storage,
        "Desktop connection state is unavailable. Restart the Desktop Client.",
    )
}

/// Answers one request. Commands that drive mpv run on the blocking pool: a
/// transition fade may sleep for up to two seconds.
async fn dispatch(state: Arc<AppState>, method: &str, params: Value) -> Reply {
    match method {
        "get_server_connection" => reply(get_server_connection(&state)),
        "test_server_connection" => {
            let origin = param(&params, "origin")?;
            reply(test_server_connection(&state, origin).await)
        }
        "save_server_connection" => {
            let origin = param(&params, "origin")?;
            reply(save_server_connection(&state, origin).await)
        }
        "desktop_http_request" => {
            let request = param(&params, "request")?;
            reply(desktop_http_request(&state, request).await)
        }
        "desktop_register_import_paths" => {
            let paths = param(&params, "paths")?;
            reply(desktop_register_import_paths(&state, paths))
        }
        "desktop_upload_import_file" => {
            let selection_id = param(&params, "selectionId")?;
            let upload_id = param(&params, "uploadId")?;
            let job_id = param(&params, "jobId")?;
            reply(desktop_upload_import_file(&state, selection_id, upload_id, job_id).await)
        }
        "desktop_cancel_import_upload" => {
            let upload_id: String = param(&params, "uploadId")?;
            reply(state.import_selections.cancel(&upload_id))
        }
        "desktop_release_import_selections" => {
            let selection_ids: Vec<String> = param(&params, "selectionIds")?;
            reply(state.import_selections.release(&selection_ids))
        }
        "get_media_proxy_url" => reply::<_, ()>(Ok(state.media_proxy.base_url())),
        "desktop_reconnect_queue_events" => {
            state.queue_events.reconnect();
            Ok(Value::Null)
        }
        "get_desktop_mpv_status" => {
            blocking(&state, |_| Ok::<_, ()>(playback::probe_mpv_status())).await
        }
        "get_desktop_playback_state" => reply(state.playback.state()),
        "desktop_playback_renderer_ready" => reply(desktop_playback_renderer_ready(&state)),
        "desktop_playback_play" => {
            let source = param(&params, "source")?;
            blocking(&state, move |state| desktop_playback_play(state, source)).await
        }
        "desktop_playback_pause" => blocking(&state, |state| state.playback.pause()).await,
        "desktop_playback_stop" => blocking(&state, |state| state.playback.stop()).await,
        "desktop_playback_toggle_play" => {
            blocking(&state, |state| state.playback.toggle_play()).await
        }
        "desktop_playback_previous" => blocking(&state, |state| state.playback.previous()).await,
        "desktop_playback_next" => blocking(&state, |state| state.playback.next()).await,
        "desktop_playback_sync_queue_context" => {
            let sources = param(&params, "sources")?;
            let current_index = param(&params, "currentIndex")?;
            blocking(&state, move |state| {
                desktop_playback_sync_queue_context(state, sources, current_index)
            })
            .await
        }
        "desktop_playback_seek" => {
            let seconds: f64 = param(&params, "seconds")?;
            blocking(&state, move |state| state.playback.seek(seconds)).await
        }
        "desktop_playback_set_volume" => {
            let value: f64 = param(&params, "value")?;
            blocking(&state, move |state| {
                update_processing(state, |processing| processing.set_software_volume(value))
            })
            .await
        }
        "desktop_playback_set_playback_rate" => {
            let rate: f64 = param(&params, "rate")?;
            blocking(&state, move |state| state.playback.set_playback_rate(rate)).await
        }
        "desktop_playback_set_stop_after_current" => {
            let enabled: bool = param(&params, "enabled")?;
            blocking(&state, move |state| {
                state.playback.set_stop_after_current(enabled)
            })
            .await
        }
        "desktop_playback_set_transition_fade" => {
            let milliseconds: u64 = param(&params, "milliseconds")?;
            blocking(&state, move |state| {
                state.playback.set_transition_fade_ms(milliseconds)
            })
            .await
        }
        "desktop_playback_set_processing_profile" => {
            let profile: ProcessingProfile = param(&params, "profile")?;
            blocking(&state, move |state| {
                update_processing(state, |processing| processing.set_profile(profile))
            })
            .await
        }
        "desktop_playback_set_replay_gain" => {
            let mode: ReplayGainMode = param(&params, "mode")?;
            blocking(&state, move |state| {
                desktop_playback_set_replay_gain(state, mode)
            })
            .await
        }
        "desktop_playback_set_equalizer_preset" => {
            let preset: EqualizerPreset = param(&params, "preset")?;
            blocking(&state, move |state| {
                update_processing(state, |processing| {
                    processing.apply_equalizer_preset(preset)
                })
            })
            .await
        }
        "desktop_playback_set_equalizer_gain" => {
            let index: usize = param(&params, "index")?;
            let value: f64 = param(&params, "value")?;
            blocking(&state, move |state| {
                update_processing(state, |processing| {
                    processing.set_equalizer_gain(index, value)
                })
            })
            .await
        }
        "desktop_playback_refresh_output_devices" => {
            blocking(&state, |state| state.playback.refresh_output_devices()).await
        }
        "desktop_playback_select_direct_alsa_output" => {
            let device_id = param(&params, "deviceId")?;
            blocking(&state, move |state| {
                desktop_playback_select_direct_alsa_output(state, device_id)
            })
            .await
        }
        "desktop_playback_select_exclusive_output" => {
            blocking(&state, desktop_playback_select_exclusive_output).await
        }
        "desktop_playback_fallback_to_system_output" => {
            blocking(&state, select_normal_output).await
        }
        "desktop_playback_enable_adaptive_system_rate" => {
            blocking(&state, desktop_playback_enable_adaptive_system_rate).await
        }
        "desktop_playback_toggle_shuffle" => {
            blocking(&state, |state| state.playback.toggle_shuffle()).await
        }
        "desktop_playback_cycle_repeat_mode" => {
            blocking(&state, |state| state.playback.cycle_repeat_mode()).await
        }
        "desktop_playback_quit" => {
            blocking(&state, |state| {
                dispatch_application_action(state, DesktopPlaybackAction::Quit)
            })
            .await
        }
        "shell_action" => {
            let action = shell_action(&param::<String>(&params, "action")?)?;
            blocking(&state, move |state| {
                dispatch_application_action(state, action)
            })
            .await
        }
        _ => Err(Value::String(format!(
            "Unknown desktop command '{method}'."
        ))),
    }
}

async fn blocking<T, E>(
    state: &Arc<AppState>,
    command: impl FnOnce(&AppState) -> Result<T, E> + Send + 'static,
) -> Reply
where
    T: Serialize + Send + 'static,
    E: Serialize + Send + 'static,
{
    let state = state.clone();
    runtime::spawn_blocking(move || reply(command(&state)))
        .await
        .unwrap_or_else(|_| Err(Value::String("Desktop command panicked.".to_owned())))
}

/// Shell-originated actions: tray menu items and the main window's close
/// button. They go through the same dispatch as MPRIS.
fn shell_action(name: &str) -> Result<DesktopPlaybackAction, Value> {
    Ok(match name {
        "open-main-window" => DesktopPlaybackAction::OpenMainWindow,
        "close-main-window" => DesktopPlaybackAction::CloseMainWindow,
        "toggle-play" => DesktopPlaybackAction::TogglePlay,
        "previous" => DesktopPlaybackAction::Previous,
        "next" => DesktopPlaybackAction::Next,
        "toggle-mini-player" => DesktopPlaybackAction::ToggleMiniPlayer,
        "quit" => DesktopPlaybackAction::Quit,
        _ => return Err(Value::String(format!("Unknown shell action '{name}'."))),
    })
}

fn dispatch_application_action(
    state: &AppState,
    action: DesktopPlaybackAction,
) -> Result<(), PlaybackCommandError> {
    dispatch_desktop_playback_action(
        action,
        &state.playback,
        &state.playback_lifecycle,
        &state.playback_snapshot_store,
        &RpcDesktopPlaybackShell { state },
    )
}

/// Window and lifetime requests become `shell:*` events for Electron.
struct RpcDesktopPlaybackShell<'a> {
    state: &'a AppState,
}

impl DesktopPlaybackShell for RpcDesktopPlaybackShell<'_> {
    fn show_main_window(&self) -> Result<(), String> {
        self.state.output.emit(SHELL_SHOW_MAIN_WINDOW, ());
        Ok(())
    }

    fn hide_main_window(&self) -> Result<(), String> {
        self.state.output.emit(SHELL_HIDE_MAIN_WINDOW, ());
        Ok(())
    }

    fn set_software_volume(&self, volume: f64) -> Result<(), String> {
        update_processing(self.state, |processing| {
            processing.set_software_volume(volume)
        })
        .map(|_| ())
        .map_err(|error| error.message)
    }

    fn toggle_mini_window(&self) -> Result<(), String> {
        self.state.output.emit(SHELL_TOGGLE_MINI_WINDOW, ());
        Ok(())
    }

    fn exit(&self) {
        self.state.exiting.store(true, Ordering::SeqCst);
        self.state.output.emit(SHELL_EXIT, ());
        let _ = self.state.exit.send(());
    }
}

/// Publishes a Playback Session to the renderers, the tray, the window title
/// and MPRIS, coalescing playhead-only progress.
struct PlaybackPublisher {
    output: Arc<Output>,
    throttle: Mutex<PlayheadThrottle>,
    tray: PlaybackTray,
    window_title: Mutex<String>,
    mpris: MprisBridge,
}

impl PlaybackPublisher {
    fn publish(&self, state: PlaybackSessionState) {
        let should_publish = self
            .throttle
            .lock()
            .map(|mut throttle| throttle.should_publish(&state))
            .unwrap_or(true);
        if !should_publish {
            return;
        }
        let is_playing = state.status.is_active();
        if let Some(update) = self.tray.update(
            state.source.as_ref(),
            is_playing,
            PlaybackPosition {
                current_seconds: state.current_time,
                duration_seconds: state.duration,
            },
        ) {
            self.output.emit(SHELL_TRAY, update);
        }
        self.publish_window_title(&state, is_playing);
        self.mpris.update(state.clone());
        self.output.emit(PLAYBACK_STATE_EVENT, state);
    }

    /// Sets the main window title from the playing source, only when it changes.
    fn publish_window_title(&self, state: &PlaybackSessionState, is_playing: bool) {
        let title =
            PlaybackTrayView::from_playback(state.source.as_ref(), is_playing).window_title();
        let Ok(mut last) = self.window_title.lock() else {
            return;
        };
        if *last == title {
            return;
        }
        self.output
            .emit(SHELL_WINDOW_TITLE, json!({ "title": title }));
        *last = title;
    }
}

impl AppState {
    fn start(
        config_directory: &Path,
        output: Arc<Output>,
        exit: mpsc::Sender<()>,
    ) -> Result<Arc<Self>, String> {
        let adaptive_system_rate = AdaptiveSystemRateController::recover_startup_if_marked(
            platform_rate_adapter(config_directory),
            Arc::new(FileAdaptiveCleanupMarker::new(
                config_directory.join(ADAPTIVE_CLEANUP_MARKER_FILE_NAME),
            )),
        )?;
        let store = ConnectionStore::new(config_directory.join(CONNECTION_FILE_NAME));
        let playback_snapshot_store =
            PlaybackSnapshotStore::new(config_directory.join(PLAYBACK_SNAPSHOT_FILE_NAME));
        let saved_origin = store.load().map_err(|error| error.message)?;
        let bridge = Arc::new(HttpBridge::new().map_err(|error| error.message)?);
        let saved_playback = runtime::block_on(playback_restore::load_saved_playback(
            &playback_snapshot_store,
            &bridge,
            saved_origin.as_ref(),
        ))
        .map_err(|error| error.to_string())?;
        let origin = Arc::new(RwLock::new(saved_origin));
        let media_proxy =
            MediaProxy::start(bridge.clone(), origin.clone()).map_err(|error| error.message)?;
        let saved_playback = saved_playback
            .map(|snapshot| snapshot.rebind_media_proxy(media_proxy.base_url()))
            .transpose()
            .map_err(|error| error.to_string())?;
        let playback_lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
        let active_output_resolver: Arc<dyn ActiveOutputResolver> =
            Arc::new(PlatformActiveOutputResolver::new());
        let mut processing =
            ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
                config_directory.join(PROCESSING_SETTINGS_FILE_NAME),
            )))?;
        if processing.output_mode() == OutputMode::AdaptiveSystemRate {
            processing.set_output_mode(OutputMode::System)?;
        }
        let device_attenuation_record = config_directory.join(DEVICE_ATTENUATION_FILE_NAME);
        recover_left_device_attenuation(&device_attenuation_record, &mut processing);

        // MPRIS commands can arrive before the state exists, and must not
        // keep it alive at exit; they reach it through a weak reference.
        let mpris_state: Arc<OnceLock<Weak<AppState>>> = Arc::new(OnceLock::new());
        let mpris_target = mpris_state.clone();
        let mpris = MprisBridge::start(
            Arc::new(move |action| {
                let Some(state) = mpris_target.get().and_then(Weak::upgrade) else {
                    return;
                };
                if let Err(error) = dispatch_application_action(&state, action) {
                    eprintln!("MPRIS action {action:?} failed: {}", error.message);
                }
            }),
            media_proxy.base_url().to_owned(),
        );
        let publisher = PlaybackPublisher {
            output: output.clone(),
            throttle: Mutex::new(PlayheadThrottle::default()),
            tray: PlaybackTray::default(),
            window_title: Mutex::new(String::new()),
            mpris,
        };
        if let Some(update) = publisher
            .tray
            .update(None, false, PlaybackPosition::default())
        {
            output.emit(SHELL_TRAY, update);
        }
        let playback = PlaybackController::start_default_with_lifecycle(
            playback_lifecycle.clone(),
            Arc::new(PlatformPathObserver::new()),
            adaptive_system_rate,
            move |state| publisher.publish(state),
        )?;
        playback.refresh_output_devices().unwrap_or_else(|error| {
            eprintln!(
                "Raw ALSA hardware startup enumeration failed: {}",
                error.message
            );
            playback.state().unwrap_or_default()
        });
        if processing.output_mode() == OutputMode::DirectAlsa {
            restore_saved_exclusive_output(
                &playback,
                &mut processing,
                active_output_resolver.as_ref(),
            );
        }
        if let Some(snapshot) = saved_playback.as_ref() {
            playback
                .restore_paused(snapshot)
                .map_err(|error| error.message)?;
        }
        playback
            .apply_processing(processing.state().clone(), &processing.mpv_configuration())
            .map_err(|error| error.message)?;
        let queue_event_output = output.clone();
        let queue_error_output = output.clone();
        let queue_events = QueueEventService::start(
            bridge.clone(),
            origin.clone(),
            move |event| queue_event_output.emit(QUEUE_INVALIDATED_EVENT, event),
            move |message| queue_error_output.emit(QUEUE_EVENTS_ERROR_EVENT, message),
        );
        let state = Arc::new(AppState {
            playback,
            processing: Mutex::new(processing),
            active_output_resolver,
            playback_lifecycle,
            playback_snapshot_store,
            bridge,
            store,
            origin,
            media_proxy,
            queue_events,
            import_selections: ImportSelectionStore::default(),
            output,
            exiting: AtomicBool::new(false),
            exit,
            device_volume: Arc::new(PlatformDeviceVolume {}),
            device_attenuation: Mutex::new(None),
            device_attenuation_record,
        });
        let _ = mpris_state.set(Arc::downgrade(&state));
        hand_off_restored_exclusive_volume(&state);
        Ok(state)
    }
}

/// Runs the core until the shell asks it to quit or closes stdin. Settings
/// files live in `config_directory`.
pub fn run(config_directory: &Path) -> Result<(), String> {
    let output = Arc::new(Output::stdout());
    let (exit_sender, exit_receiver) = mpsc::channel();
    let state = AppState::start(config_directory, output.clone(), exit_sender.clone())?;
    output.emit(
        SHELL_READY,
        json!({ "mediaProxyUrl": state.media_proxy.base_url() }),
    );

    // The reader holds only a weak reference, so the state can be dropped
    // (stopping mpv and restoring the system output) while it waits on stdin.
    let reader_state = Arc::downgrade(&state);
    let reader_output = output.clone();
    std::thread::Builder::new()
        .name("earthly-core-stdin".to_owned())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let request: Request = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(error) => {
                        eprintln!("Desktop core ignored a malformed request: {error}");
                        continue;
                    }
                };
                let Some(state) = reader_state.upgrade() else {
                    break;
                };
                let output = reader_output.clone();
                runtime::spawn(async move {
                    let reply = dispatch(state, &request.method, request.params).await;
                    output.respond(request.id, reply);
                });
            }
            let _ = exit_sender.send(());
        })
        .map_err(|error| format!("Desktop core stdin reader could not start: {error}"))?;

    let _ = exit_receiver.recv();
    if !state.exiting.load(Ordering::SeqCst)
        && let Err(error) = dispatch_application_action(&state, DesktopPlaybackAction::Quit)
    {
        eprintln!(
            "Desktop core shutdown could not save playback: {}",
            error.message
        );
    }
    release(state);
    Ok(())
}

/// A saved Exclusive Output reopens with the software volume written at exit,
/// so it moves onto the device again before playback starts.
fn hand_off_restored_exclusive_volume(state: &AppState) {
    let Ok(mut processing) = state.processing.lock() else {
        return;
    };
    let route = processing
        .selected_output_device_id()
        .and_then(VolumeRoute::for_alsa_device);
    let (Some(route), Ok(playback_state)) = (route, state.playback.state()) else {
        return;
    };
    if processing.output_mode() == OutputMode::DirectAlsa {
        hand_off_volume_to_device(state, &mut processing, route, playback_state);
    }
}

/// Drops the state once in-flight commands let go of it, so its teardown
/// (mpv, the media proxy, the Adaptive System Rate restore) runs before exit.
fn release(mut state: Arc<AppState>) {
    // The listener's software volume is saved for the next start, and the
    // system output gets its level back.
    if let Ok(mut processing) = state.processing.lock() {
        return_device_volume(&state, &mut processing);
    }
    for _ in 0..100 {
        match Arc::try_unwrap(state) {
            Ok(state) => {
                drop(state);
                return;
            }
            Err(shared) => {
                state = shared;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    eprintln!("Desktop core exited while a command still held its state.");
}

#[cfg(test)]
mod tests {
    use super::{
        DeviceAttenuationRecord, RecordedRoute, returned_volume, shell_action,
        validate_playback_source,
    };
    use crate::playback_app_actions::DesktopPlaybackAction;
    use serde_json::json;

    #[test]
    fn a_hand_off_record_keeps_its_route_target_and_the_listeners_level() {
        let record = DeviceAttenuationRecord {
            route: RecordedRoute::SystemSink,
            target: Some("alsa_output.usb-dac".to_owned()),
            applied_db: -3.5,
            software_volume: 0.68,
        };
        let text = serde_json::to_string(&record).expect("record JSON");

        assert!(text.contains("\"route\":\"system-sink\""), "{text}");
        let read: DeviceAttenuationRecord = serde_json::from_str(&text).expect("read back");
        assert_eq!(read.target.as_deref(), Some("alsa_output.usb-dac"));
        assert_eq!(read.applied_db, -3.5);
        assert_eq!(read.software_volume, 0.68);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_raw_alsa_hand_off_needs_no_device_undo() {
        // PipeWire restores the card's own volume; only mpv's level comes back.
        let record = DeviceAttenuationRecord {
            route: RecordedRoute::AlsaCard,
            target: None,
            applied_db: -3.5,
            software_volume: 0.68,
        };

        assert_eq!(super::undo_recorded_hand_off(&record), Ok(true));
    }

    #[test]
    fn native_playback_rejects_urls_outside_private_media_proxy() {
        let source = json!({
            "type": "track",
            "playbackUrl": "https://attacker.example/track.flac"
        });

        assert!(validate_playback_source(&source, "http://127.0.0.1:43129/private-token").is_err());
    }

    #[test]
    fn returned_volume_keeps_the_level_heard_during_the_hand_off() {
        // Untouched: mpv sat at full while the device carried 20%.
        assert!((returned_volume(1.0, 0.2) - 0.2).abs() < 1e-9);
        // 10% chosen during the hand-off on a 20% device: 2% is heard.
        assert!((returned_volume(0.1, 0.2) - 0.02).abs() < 1e-9);
    }

    #[test]
    fn shell_actions_map_only_known_names() {
        assert_eq!(
            shell_action("close-main-window"),
            Ok(DesktopPlaybackAction::CloseMainWindow)
        );
        assert_eq!(shell_action("quit"), Ok(DesktopPlaybackAction::Quit));
        assert!(shell_action("seek").is_err());
    }
}
