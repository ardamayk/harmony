pub mod adaptive_system_rate;
pub mod connection;
pub mod desktop_import;
mod exclusive_output;
pub mod media_proxy;
pub mod mini_window;
mod mpris;
pub mod output_device;
mod playback;
mod playback_app_actions;
mod playback_lifecycle;
mod playback_navigation;
mod playback_publish;
mod playback_restore;
#[cfg(test)]
mod playback_test_support;
mod playback_tray;
pub mod processing;
mod queue_events;
pub mod telemetry;

use adaptive_system_rate::{
    AdaptiveSystemRateController, CommandPipeWireRateAdapter, FileAdaptiveCleanupMarker,
};
use connection::{
    ConnectionCheck, ConnectionError, ConnectionErrorCode, ConnectionStore, HttpBridge,
    HttpRequest, HttpResponse, ServerOrigin,
};
use desktop_import::{
    DesktopImportError, ImportSelection, ImportSelectionStore, ImportUploadProgress,
    SUPPORTED_EXTENSIONS, upload_selection,
};
use media_proxy::MediaProxy;
use mini_window::MiniWindowStore;
use mpris::MprisBridge;
use output_device::{
    ActiveOutputError, ActiveOutputResolver, CommandPipeWireActiveOutputResolver, OutputDevice,
};
use playback::{
    OutputDeviceIssueCode, PlaybackCommandError, PlaybackController, PlaybackSessionState,
};
use playback_app_actions::{
    DesktopPlaybackAction, DesktopPlaybackShell, dispatch_desktop_playback_action,
};
use playback_lifecycle::{PlaybackLifecycle, PlaybackSnapshotStore};
use playback_publish::PlayheadThrottle;
use playback_tray::{
    PlaybackPosition, PlaybackTray, PlaybackTrayView, TRAY_MINI_ID, TRAY_NEXT_ID, TRAY_OPEN_ID,
    TRAY_PREVIOUS_ID, TRAY_QUIT_ID, TRAY_TOGGLE_ID,
};
use processing::{
    EqualizerPreset, FileProcessingSettingsStorage, OutputMode, ProcessingController,
    ProcessingProfile, ReplayGainMode, mpv_configuration_for,
};
use queue_events::QueueEventService;
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};
use telemetry::CommandPipeWireObserver;

const CONNECTION_FILE_NAME: &str = "server-connection.json";
const PLAYBACK_SNAPSHOT_FILE_NAME: &str = "playback-session.json";
const PROCESSING_SETTINGS_FILE_NAME: &str = "processing-settings.json";
const ADAPTIVE_CLEANUP_MARKER_FILE_NAME: &str = "adaptive-system-rate.cleanup-required";
const MINI_WINDOW_FILE_NAME: &str = "mini-window.json";
const PLAYBACK_STATE_EVENT: &str = "desktop-playback-state";
const CONNECTION_CHANGED_EVENT: &str = "server-connection-changed";
const QUEUE_EVENTS_ERROR_EVENT: &str = "desktop-queue-events-error";
const QUEUE_INVALIDATED_EVENT: &str = "desktop-queue-invalidated";

struct AppState {
    playback: PlaybackController,
    processing: Mutex<ProcessingController>,
    active_output_resolver: Arc<dyn ActiveOutputResolver>,
    playback_lifecycle: Arc<Mutex<PlaybackLifecycle>>,
    playback_snapshot_store: PlaybackSnapshotStore,
    _playback_tray: PlaybackTray,
    bridge: Arc<HttpBridge>,
    store: ConnectionStore,
    origin: Arc<RwLock<Option<ServerOrigin>>>,
    media_proxy: MediaProxy,
    queue_events: QueueEventService,
    import_selections: ImportSelectionStore,
    mini_window_store: MiniWindowStore,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerConnection {
    origin: String,
}

#[tauri::command]
fn get_server_connection(
    state: State<'_, AppState>,
) -> Result<Option<ServerConnection>, ConnectionError> {
    let origin = state.origin.read().map_err(|_| state_error())?;
    Ok(origin.as_ref().map(|origin| ServerConnection {
        origin: origin.as_str().to_owned(),
    }))
}

#[tauri::command]
async fn test_server_connection(
    state: State<'_, AppState>,
    origin: String,
) -> Result<ConnectionCheck, ConnectionError> {
    let origin = ServerOrigin::parse(&origin)?;
    state.bridge.test_server(&origin).await
}

#[tauri::command]
async fn save_server_connection(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    origin: String,
) -> Result<ConnectionCheck, ConnectionError> {
    let origin = ServerOrigin::parse(&origin)?;
    let check = state.bridge.test_server(&origin).await?;
    state.store.save(&origin)?;
    *state.origin.write().map_err(|_| state_error())? = Some(origin);
    state.queue_events.reconnect();
    app.emit(CONNECTION_CHANGED_EVENT, &check)
        .map_err(|_| state_error())?;
    Ok(check)
}

#[tauri::command]
async fn desktop_http_request(
    state: State<'_, AppState>,
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

#[tauri::command]
async fn desktop_select_import_files(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ImportSelection>, DesktopImportError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Audio", SUPPORTED_EXTENSIONS)
        .pick_files(move |paths| {
            if sender.send(paths).is_err() {
                eprintln!("Desktop import file picker receiver closed");
            }
        });
    let paths = receiver
        .await
        .map_err(|_| desktop_import_state_error())?
        .unwrap_or_default();
    let paths = import_dialog_paths(paths)?;
    state.import_selections.register_paths(paths)
}

#[tauri::command]
async fn desktop_select_import_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ImportSelection>, DesktopImportError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |path| {
        if sender.send(path).is_err() {
            eprintln!("Desktop import folder picker receiver closed");
        }
    });
    let paths = receiver
        .await
        .map_err(|_| desktop_import_state_error())?
        .into_iter()
        .collect::<Vec<_>>();
    let paths = import_dialog_paths(paths)?;
    state.import_selections.register_paths(paths)
}

fn import_dialog_paths(
    paths: Vec<FilePath>,
) -> Result<Vec<std::path::PathBuf>, DesktopImportError> {
    paths
        .into_iter()
        .map(|path| {
            path.into_path().map_err(|_| DesktopImportError {
                code: "invalid_selection",
                message: "Native picker returned a file path that cannot be read.".to_owned(),
            })
        })
        .collect()
}

#[tauri::command]
async fn desktop_upload_import_file(
    state: State<'_, AppState>,
    selection_id: String,
    upload_id: String,
    job_id: String,
    on_progress: tauri::ipc::Channel<ImportUploadProgress>,
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

#[tauri::command]
fn desktop_cancel_import_upload(
    state: State<'_, AppState>,
    upload_id: String,
) -> Result<(), DesktopImportError> {
    state.import_selections.cancel(&upload_id)
}

#[tauri::command]
fn desktop_release_import_selections(
    state: State<'_, AppState>,
    selection_ids: Vec<String>,
) -> Result<(), DesktopImportError> {
    state.import_selections.release(&selection_ids)
}

fn desktop_import_state_error() -> DesktopImportError {
    DesktopImportError {
        code: "state_unavailable",
        message: "Desktop import state is unavailable.".to_owned(),
    }
}

#[tauri::command]
fn get_media_proxy_url(state: State<'_, AppState>) -> String {
    state.media_proxy.base_url().to_owned()
}

#[tauri::command]
fn desktop_reconnect_queue_events(state: State<'_, AppState>) {
    state.queue_events.reconnect();
}

#[tauri::command]
fn get_desktop_playback_state(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.state()
}

#[tauri::command]
fn desktop_playback_renderer_ready(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state
        .playback_lifecycle
        .lock()
        .map_err(|_| PlaybackCommandError::new("Playback lifecycle state is unavailable."))?
        .renderer_attached();
    let playback_state = state.playback.state()?;
    app.emit(PLAYBACK_STATE_EVENT, &playback_state)
        .map_err(|_| PlaybackCommandError::new("Playback state event could not be published."))?;
    Ok(playback_state)
}

#[tauri::command]
fn get_desktop_mpv_status() -> playback::MpvStatus {
    playback::probe_mpv_status()
}

/// External links (Spotify, Last.fm, ...) leave the Desktop Client and open in
/// the system browser. Only web URLs are accepted so the renderer cannot hand
/// arbitrary schemes to the operating system.
#[tauri::command]
fn desktop_open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let url = validate_external_url(&url)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|error| error.to_string())
}

fn validate_external_url(raw: &str) -> Result<url::Url, String> {
    let parsed =
        url::Url::parse(raw).map_err(|_| "External link is not a valid URL.".to_owned())?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => Err(format!("External link scheme '{scheme}' is not allowed.")),
    }
}

#[tauri::command]
fn desktop_playback_quit(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PlaybackCommandError> {
    dispatch_application_action(&app, &state, DesktopPlaybackAction::Quit)
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_play(
    state: State<'_, AppState>,
    source: Option<Value>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    if let Some(source) = source.as_ref() {
        validate_playback_source(source, state.media_proxy.base_url())?;
    }
    state.playback.play(source)
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_pause(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.pause()
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_stop(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.stop()
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_toggle_play(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.toggle_play()
}

#[tauri::command]
fn desktop_playback_sync_queue_context(
    state: State<'_, AppState>,
    sources: Vec<Value>,
    current_index: Option<usize>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    for source in &sources {
        validate_playback_source(source, state.media_proxy.base_url())?;
    }
    state.playback.sync_queue_context(sources, current_index)
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_previous(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.previous()
}

// Runs off the main thread: a transition fade may sleep for up to two seconds.
#[tauri::command]
async fn desktop_playback_next(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.next()
}

#[tauri::command]
fn desktop_playback_seek(
    state: State<'_, AppState>,
    seconds: f64,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.seek(seconds)
}

#[tauri::command]
fn desktop_open_mini_player(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PlaybackCommandError> {
    mini_window::open(&app, &state.mini_window_store).map_err(PlaybackCommandError::new)
}

#[tauri::command]
fn desktop_close_mini_player(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PlaybackCommandError> {
    mini_window::close(&app, &state.mini_window_store).map_err(PlaybackCommandError::new)
}

#[tauri::command]
fn desktop_toggle_mini_player(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PlaybackCommandError> {
    mini_window::toggle(&app, &state.mini_window_store).map_err(PlaybackCommandError::new)
}

#[tauri::command]
fn desktop_show_main_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PlaybackCommandError> {
    dispatch_application_action(&app, &state, DesktopPlaybackAction::OpenMainWindow)
}

#[tauri::command]
fn desktop_playback_set_playback_rate(
    state: State<'_, AppState>,
    rate: f64,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.set_playback_rate(rate)
}

#[tauri::command]
fn desktop_playback_set_stop_after_current(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.set_stop_after_current(enabled)
}

#[tauri::command]
fn desktop_playback_set_transition_fade(
    state: State<'_, AppState>,
    milliseconds: u64,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.set_transition_fade_ms(milliseconds)
}

#[tauri::command]
fn desktop_playback_set_volume(
    state: State<'_, AppState>,
    value: f64,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(&state, |processing| processing.set_software_volume(value))
}

#[tauri::command]
fn desktop_playback_set_processing_profile(
    state: State<'_, AppState>,
    profile: ProcessingProfile,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(&state, |processing| processing.set_profile(profile))
}

#[tauri::command]
fn desktop_playback_set_replay_gain(
    state: State<'_, AppState>,
    mode: ReplayGainMode,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(&state, |processing| {
        if mode == ReplayGainMode::Off {
            processing.disable_replay_gain()
        } else {
            processing.enable_replay_gain(mode)
        }
    })
}

#[tauri::command]
fn desktop_playback_set_equalizer_preset(
    state: State<'_, AppState>,
    preset: EqualizerPreset,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(&state, |processing| {
        processing.apply_equalizer_preset(preset)
    })
}

#[tauri::command]
fn desktop_playback_set_equalizer_gain(
    state: State<'_, AppState>,
    index: usize,
    value: f64,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    update_processing(&state, |processing| {
        processing.set_equalizer_gain(index, value)
    })
}

#[tauri::command]
fn desktop_playback_refresh_output_devices(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.refresh_output_devices()
}

#[tauri::command]
fn desktop_playback_select_direct_alsa_output(
    state: State<'_, AppState>,
    device_id: String,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
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
        &state,
        &mut processing,
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

#[tauri::command]
fn desktop_playback_fallback_to_system_output(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    select_normal_output(&state)
}

#[tauri::command]
fn desktop_playback_select_exclusive_output(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let device = match state.active_output_resolver.resolve() {
        Ok(device) => device,
        Err(error) => {
            select_normal_output(&state)?;
            return Err(PlaybackCommandError::new(active_output_error_message(
                &error,
            )));
        }
    };
    select_exclusive_output(&state, device)
}

#[tauri::command]
fn desktop_playback_enable_adaptive_system_rate(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    let mut processing = state
        .processing
        .lock()
        .map_err(|_| PlaybackCommandError::new("Output Mode settings are unavailable."))?;
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
    Ok(restore_remembered_volume(
        &state,
        &mut processing,
        playback_state,
    ))
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
    Ok(restore_remembered_volume(
        state,
        &mut processing,
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

#[tauri::command]
fn desktop_playback_toggle_shuffle(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.toggle_shuffle()
}

#[tauri::command]
fn desktop_playback_cycle_repeat_mode(
    state: State<'_, AppState>,
) -> Result<PlaybackSessionState, PlaybackCommandError> {
    state.playback.cycle_repeat_mode()
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

/// Sets the main window title from the playing source, only when it changes.
fn apply_window_title(
    app: &tauri::AppHandle,
    last_title: &Mutex<String>,
    state: &PlaybackSessionState,
    is_playing: bool,
) {
    let title = PlaybackTrayView::from_playback(state.source.as_ref(), is_playing).window_title();
    let Ok(mut last) = last_title.lock() else {
        return;
    };
    if *last == title {
        return;
    }
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window.set_title(&title)
    {
        eprintln!("Desktop window title update failed: {error}");
        return;
    }
    *last = title;
}

fn handle_tray_menu_event(app: &tauri::AppHandle, id: &str) {
    let action = match id {
        TRAY_OPEN_ID => DesktopPlaybackAction::OpenMainWindow,
        TRAY_TOGGLE_ID => DesktopPlaybackAction::TogglePlay,
        TRAY_PREVIOUS_ID => DesktopPlaybackAction::Previous,
        TRAY_NEXT_ID => DesktopPlaybackAction::Next,
        TRAY_MINI_ID => DesktopPlaybackAction::ToggleMiniPlayer,
        TRAY_QUIT_ID => DesktopPlaybackAction::Quit,
        _ => return,
    };
    let state = app.state::<AppState>();
    let result = dispatch_application_action(app, &state, action).map_err(|error| error.message);
    if let Err(error) = result {
        eprintln!("Desktop playback tray action '{id}' failed: {error}");
    }
}

fn dispatch_application_action(
    app: &tauri::AppHandle,
    state: &AppState,
    action: DesktopPlaybackAction,
) -> Result<(), PlaybackCommandError> {
    dispatch_desktop_playback_action(
        action,
        &state.playback,
        &state.playback_lifecycle,
        &state.playback_snapshot_store,
        &TauriDesktopPlaybackShell { app },
    )
}

fn handle_main_window_close(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    api.prevent_close();
    let state = window.state::<AppState>();
    if let Err(error) = dispatch_application_action(
        window.app_handle(),
        &state,
        DesktopPlaybackAction::CloseMainWindow,
    ) {
        eprintln!("Desktop main window close action failed: {}", error.message);
    }
}

struct TauriDesktopPlaybackShell<'a> {
    app: &'a tauri::AppHandle,
}

impl DesktopPlaybackShell for TauriDesktopPlaybackShell<'_> {
    fn show_main_window(&self) -> Result<(), String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or_else(|| "Desktop main window is unavailable.".to_owned())?;
        window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus())
            .map_err(|error| error.to_string())
    }

    fn set_software_volume(&self, volume: f64) -> Result<(), String> {
        let state = self
            .app
            .try_state::<AppState>()
            .ok_or_else(|| "Desktop playback state is unavailable.".to_owned())?;
        update_processing(&state, |processing| processing.set_software_volume(volume))
            .map(|_| ())
            .map_err(|error| error.message)
    }

    fn toggle_mini_window(&self) -> Result<(), String> {
        let state = self
            .app
            .try_state::<AppState>()
            .ok_or_else(|| "Desktop playback state is unavailable.".to_owned())?;
        mini_window::toggle(self.app, &state.mini_window_store)
    }

    fn hide_main_window(&self) -> Result<(), String> {
        self.app
            .get_webview_window("main")
            .ok_or_else(|| "Desktop main window is unavailable.".to_owned())?
            .hide()
            .map_err(|error| error.to_string())
    }

    fn exit(&self) {
        self.app.exit(0);
    }
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Err(error) = (TauriDesktopPlaybackShell { app }).show_main_window() {
                eprintln!("Desktop relaunch could not restore the main window: {error}");
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(handle_main_window_close)
        .setup(|app| {
            let config_directory = app.path().app_config_dir()?;
            let adaptive_system_rate = AdaptiveSystemRateController::recover_startup_if_marked(
                Arc::new(CommandPipeWireRateAdapter::new()),
                Arc::new(FileAdaptiveCleanupMarker::new(
                    config_directory.join(ADAPTIVE_CLEANUP_MARKER_FILE_NAME),
                )),
            )
            .map_err(std::io::Error::other)?;
            let store = ConnectionStore::new(config_directory.join(CONNECTION_FILE_NAME));
            let playback_snapshot_store =
                PlaybackSnapshotStore::new(config_directory.join(PLAYBACK_SNAPSHOT_FILE_NAME));
            let saved_origin = store.load()?;
            let bridge = Arc::new(HttpBridge::new()?);
            let saved_playback =
                tauri::async_runtime::block_on(playback_restore::load_saved_playback(
                    &playback_snapshot_store,
                    &bridge,
                    saved_origin.as_ref(),
                ))
                .map_err(std::io::Error::other)?;
            let origin = Arc::new(RwLock::new(saved_origin));
            let media_proxy = MediaProxy::start(bridge.clone(), origin.clone())?;
            let saved_playback = saved_playback
                .map(|snapshot| snapshot.rebind_media_proxy(media_proxy.base_url()))
                .transpose()
                .map_err(std::io::Error::other)?;
            let playback_lifecycle = Arc::new(Mutex::new(PlaybackLifecycle::new()));
            let active_output_resolver: Arc<dyn ActiveOutputResolver> =
                Arc::new(CommandPipeWireActiveOutputResolver::new());
            let mut processing =
                ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
                    config_directory.join(PROCESSING_SETTINGS_FILE_NAME),
                )))
                .map_err(std::io::Error::other)?;
            if processing.output_mode() == OutputMode::AdaptiveSystemRate {
                processing
                    .set_output_mode(OutputMode::System)
                    .map_err(std::io::Error::other)?;
            }
            let playback_tray = PlaybackTray::start(app.handle(), handle_tray_menu_event)?;
            let playback_event_tray = playback_tray.clone();
            let app_handle = app.handle().clone();
            let window_title = Arc::new(Mutex::new(String::new()));
            let playhead_throttle = Mutex::new(PlayheadThrottle::default());
            let mpris_app = app.handle().clone();
            let mpris = MprisBridge::start(
                Arc::new(move |action| {
                    // Commands can arrive before AppState is managed; drop those.
                    let Some(state) = mpris_app.try_state::<AppState>() else {
                        return;
                    };
                    if let Err(error) = dispatch_application_action(&mpris_app, &state, action) {
                        eprintln!("MPRIS action {action:?} failed: {}", error.message);
                    }
                }),
                media_proxy.base_url().to_owned(),
            );
            let playback = PlaybackController::start_default_with_lifecycle(
                playback_lifecycle.clone(),
                Arc::new(CommandPipeWireObserver::new()),
                adaptive_system_rate,
                move |state| {
                    let should_publish = playhead_throttle
                        .lock()
                        .map(|mut throttle| throttle.should_publish(&state))
                        .unwrap_or(true);
                    if !should_publish {
                        return;
                    }
                    let is_playing = state.status.is_active();
                    if let Err(error) = playback_event_tray.update(
                        state.source.as_ref(),
                        is_playing,
                        PlaybackPosition {
                            current_seconds: state.current_time,
                            duration_seconds: state.duration,
                        },
                    ) {
                        eprintln!("Desktop playback tray update failed: {error}");
                    }
                    apply_window_title(&app_handle, &window_title, &state, is_playing);
                    mpris.update(state.clone());
                    if let Err(error) = app_handle.emit(PLAYBACK_STATE_EVENT, state) {
                        eprintln!("Desktop playback state event failed: {error}");
                    }
                },
            )
            .map_err(std::io::Error::other)?;
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
                    .map_err(|error| std::io::Error::other(error.message))?;
            }
            playback
                .apply_processing(processing.state().clone(), &processing.mpv_configuration())
                .map_err(|error| std::io::Error::other(error.message))?;
            let queue_event_app = app.handle().clone();
            let queue_error_app = app.handle().clone();
            let queue_events = QueueEventService::start(
                bridge.clone(),
                origin.clone(),
                move |event| {
                    if let Err(error) = queue_event_app.emit(QUEUE_INVALIDATED_EVENT, event) {
                        eprintln!("Desktop Queue event emission failed: {error}");
                    }
                },
                move |message| {
                    if let Err(error) = queue_error_app.emit(QUEUE_EVENTS_ERROR_EVENT, message) {
                        eprintln!("Desktop Queue error emission failed: {error}");
                    }
                },
            );
            app.manage(AppState {
                playback,
                processing: Mutex::new(processing),
                active_output_resolver,
                playback_lifecycle,
                playback_snapshot_store,
                _playback_tray: playback_tray,
                bridge,
                store,
                origin,
                media_proxy,
                queue_events,
                import_selections: ImportSelectionStore::default(),
                mini_window_store: MiniWindowStore::new(
                    config_directory.join(MINI_WINDOW_FILE_NAME),
                ),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_server_connection,
            test_server_connection,
            save_server_connection,
            desktop_http_request,
            desktop_select_import_files,
            desktop_select_import_folder,
            desktop_upload_import_file,
            desktop_cancel_import_upload,
            desktop_release_import_selections,
            get_media_proxy_url,
            desktop_reconnect_queue_events,
            get_desktop_playback_state,
            get_desktop_mpv_status,
            desktop_open_external_url,
            desktop_playback_renderer_ready,
            desktop_playback_quit,
            desktop_playback_play,
            desktop_playback_pause,
            desktop_playback_stop,
            desktop_playback_toggle_play,
            desktop_playback_sync_queue_context,
            desktop_playback_previous,
            desktop_playback_next,
            desktop_playback_seek,
            desktop_playback_set_volume,
            desktop_open_mini_player,
            desktop_close_mini_player,
            desktop_toggle_mini_player,
            desktop_show_main_window,
            desktop_playback_set_playback_rate,
            desktop_playback_set_stop_after_current,
            desktop_playback_set_transition_fade,
            desktop_playback_set_processing_profile,
            desktop_playback_set_replay_gain,
            desktop_playback_set_equalizer_preset,
            desktop_playback_set_equalizer_gain,
            desktop_playback_refresh_output_devices,
            desktop_playback_select_direct_alsa_output,
            desktop_playback_select_exclusive_output,
            desktop_playback_fallback_to_system_output,
            desktop_playback_enable_adaptive_system_rate,
            desktop_playback_toggle_shuffle,
            desktop_playback_cycle_repeat_mode
        ])
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::{validate_external_url, validate_playback_source};

    #[test]
    fn external_links_accept_only_web_urls() {
        assert!(validate_external_url("https://open.spotify.com/search/x").is_ok());
        assert!(validate_external_url("http://example.com").is_ok());
        assert!(validate_external_url("file:///etc/passwd").is_err());
        assert!(validate_external_url("javascript:alert(1)").is_err());
        assert!(validate_external_url("not a url").is_err());
    }
    use serde_json::json;

    #[test]
    fn native_playback_rejects_urls_outside_private_media_proxy() {
        let source = json!({
            "type": "track",
            "playbackUrl": "https://attacker.example/track.flac"
        });

        assert!(validate_playback_source(&source, "http://127.0.0.1:43129/private-token").is_err());
    }
}
