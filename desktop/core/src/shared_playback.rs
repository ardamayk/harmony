//! Shared Playback State for the Desktop core (ADR 0040): the core, not the
//! renderer, reports the local Player to the Music Server and obeys remote
//! commands, so Background Playback keeps working with the main window closed
//! (ADR 0009). Snapshots arrive on the Queue event stream and leave through
//! the same HTTP bridge; without the server's `playback.shared-session.v1`
//! capability nothing is sent.

use crate::connection::{
    ConnectionError, ConnectionErrorCode, HttpBridge, HttpRequest, ServerOrigin,
};
use crate::playback::{PlaybackController, PlaybackSessionState};
use crate::rpc::Output;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub(crate) const SHARED_SESSION_CAPABILITY: &str = "playback.shared-session.v1";

const STATE_COMMANDS_PATH: &str = "/api/v1/playback/state/commands";
const STATE_PATH: &str = "/api/v1/playback/state";
/// The snapshot event forwarded to renderers; named again in `DESKTOP_EVENTS`
/// in the Desktop bridge.
pub(crate) const SHARED_PLAYBACK_STATE_EVENT: &str = "desktop-shared-playback-state";
/// Persisted beside the Playback Session Snapshot, so a device keeps its
/// identity across restarts.
pub(crate) const DEVICE_FILE_NAME: &str = "shared-playback-device.json";
pub(crate) const HEARTBEAT_MS: u64 = 5_000;
/// A playhead further from the expected one than this is a jump to report or
/// an obeyed seek.
pub(crate) const PLAYHEAD_JUMP_SECONDS: f64 = 1.5;
/// How long the final report at quit may hold the process before it exits;
/// the shell kills a core that takes 5 s.
const SHUTDOWN_REPORT_TIMEOUT: Duration = Duration::from_secs(2);
/// How often the loop reconsiders a heartbeat while the Player is quiet.
const HEARTBEAT_TICK: Duration = Duration::from_secs(1);
const MAX_DEVICE_NAME_LENGTH: usize = 64;

/// This device's identity in the Shared Playback State: a stable UUID and a
/// name other clients show, "Harmony on {hostname}".
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SharedPlaybackDevice {
    pub(crate) id: String,
    pub(crate) name: String,
}

impl SharedPlaybackDevice {
    fn new(id: String, hostname: &str) -> Self {
        let full_name = format!("Harmony on {}", hostname.trim());
        Self {
            id,
            name: full_name.chars().take(MAX_DEVICE_NAME_LENGTH).collect(),
        }
    }
}

/// Loads the persisted device identity, creating one on first run. An
/// unreadable or malformed record reads as a new device.
pub(crate) struct SharedPlaybackDeviceStore {
    path: PathBuf,
}

impl SharedPlaybackDeviceStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load_or_create(&self) -> SharedPlaybackDevice {
        if let Ok(persisted) = self.load()
            && uuid::Uuid::parse_str(&persisted.device_id).is_ok()
        {
            return SharedPlaybackDevice::new(persisted.device_id, &device_hostname());
        }
        let device =
            SharedPlaybackDevice::new(uuid::Uuid::new_v4().to_string(), &device_hostname());
        // An identity for this run only still beats a core that will not start.
        if let Err(error) = self.save(&device) {
            eprintln!("Shared Playback device identity could not be saved: {error}");
        }
        device
    }

    fn load(&self) -> Result<PersistedDevice, String> {
        let contents = fs::read_to_string(&self.path).map_err(|error| error.to_string())?;
        serde_json::from_str(&contents).map_err(|error| error.to_string())
    }

    fn save(&self, device: &SharedPlaybackDevice) -> Result<(), String> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let contents = serde_json::to_vec_pretty(&PersistedDevice {
            device_id: device.id.clone(),
        })
        .map_err(|error| error.to_string())?;
        let temporary_path = self.path.with_extension("json.tmp");
        fs::write(&temporary_path, contents).map_err(|error| error.to_string())?;
        fs::rename(&temporary_path, &self.path).map_err(|error| error.to_string())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedDevice {
    device_id: String,
}

#[cfg(windows)]
fn device_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

#[cfg(not(windows))]
fn device_hostname() -> String {
    let mut buffer = [0_u8; 256];
    let resolved = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    let hostname = (resolved == 0)
        .then(|| {
            let end = buffer
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(buffer.len());
            String::from_utf8(buffer[..end].to_vec())
                .ok()
                .map(|name| name.trim().to_owned())
        })
        .flatten()
        .filter(|name| !name.is_empty());
    hostname.unwrap_or_else(|| "localhost".to_owned())
}

/// One full snapshot of the Shared Playback State, as the Music Server
/// publishes it on the event stream and answers commands with.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharedPlaybackSnapshot {
    pub(crate) revision: u64,
    pub(crate) queue_item_id: Option<String>,
    pub(crate) track_id: Option<String>,
    pub(crate) playing: bool,
    pub(crate) position_ms: u64,
    pub(crate) position_at_ms: u64,
    pub(crate) active_device_id: Option<String>,
    pub(crate) active_device_name: Option<String>,
    pub(crate) remote_command_revision: u64,
    pub(crate) skip_request: Option<SkipRequest>,
    pub(crate) server_time_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkipRequest {
    pub(crate) revision: u64,
    pub(crate) direction: SkipDirection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SkipDirection {
    Next,
    Previous,
}

/// The playhead as this device sees it, distilled from a Playback Session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocalProgress {
    pub(crate) playing: bool,
    pub(crate) position_seconds: f64,
    pub(crate) source_key: Option<String>,
    pub(crate) queue_item_id: Option<String>,
    pub(crate) track_id: Option<String>,
}

/// Reads the local Player state for reporting. Only Track sources carry
/// identifiers; every other kind reports null IDs (ADR 0040).
pub(crate) fn local_progress(state: &PlaybackSessionState) -> LocalProgress {
    let (queue_item_id, track_id) = reported_ids(state.source.as_ref());
    LocalProgress {
        playing: state.status.is_active(),
        position_seconds: state.current_time,
        source_key: source_key(state.source.as_ref()),
        queue_item_id,
        track_id,
    }
}

fn reported_ids(source: Option<&Value>) -> (Option<String>, Option<String>) {
    let Some(source) =
        source.filter(|source| source.get("type").and_then(Value::as_str) == Some("track"))
    else {
        return (None, None);
    };
    let queue_item_id = source
        .get("queueItemId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let track_id = source
        .pointer("/track/id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    (queue_item_id, track_id)
}

fn source_key(source: Option<&Value>) -> Option<String> {
    let source = source?;
    let kind = source.get("type").and_then(Value::as_str)?;
    let id = match kind {
        "track" => source.pointer("/track/id")?.as_str()?,
        "radio-station" => source.pointer("/station/id")?.as_str()?,
        "catalog-preview" => source.pointer("/result/stationUuid")?.as_str()?,
        "external-track" => source.pointer("/externalTrack/id")?.as_str()?,
        _ => return None,
    };
    let queue_item_id = source
        .get("queueItemId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!("{kind}:{id}:{queue_item_id}"))
}

/// Why a report goes out now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReportReason {
    PlayPause,
    SourceChange,
    PlayheadJump,
    Heartbeat,
}

#[derive(Clone, Debug, PartialEq)]
struct ReportedProgress {
    playing: bool,
    position_seconds: f64,
    source_key: Option<String>,
    reported_at_ms: u64,
}

/// What has been reported, so the next trigger is known.
#[derive(Default)]
pub(crate) struct ReporterState {
    last: Option<ReportedProgress>,
    /// The last progress with a source, for when the Player stops.
    loaded: Option<LocalProgress>,
    /// The last report failed: try again on the heartbeat, even while paused.
    unsent: bool,
}

impl ReporterState {
    /// Decides whether the local progress needs a report now, and records it
    /// when it does. Pure in `now_ms`, so tests drive the clock.
    pub(crate) fn next_report(
        &mut self,
        progress: &LocalProgress,
        now_ms: u64,
    ) -> Option<ReportReason> {
        if progress.source_key.is_some() {
            self.loaded = Some(progress.clone());
        }
        let Some(last) = self.last.as_ref() else {
            // Becoming Active told the server all of this already, and a
            // restored playhead must not overwrite the shared one.
            if progress.source_key.is_some() {
                self.last = Some(ReportedProgress::of(progress, now_ms));
            }
            return None;
        };
        let heartbeat_due = (progress.playing || self.unsent)
            && now_ms.saturating_sub(last.reported_at_ms) >= HEARTBEAT_MS;
        let reason = if last.playing != progress.playing {
            Some(ReportReason::PlayPause)
        } else if last.source_key != progress.source_key {
            Some(ReportReason::SourceChange)
        } else if (progress.position_seconds - last.expected_seconds(now_ms)).abs()
            > PLAYHEAD_JUMP_SECONDS
        {
            Some(ReportReason::PlayheadJump)
        } else if heartbeat_due {
            Some(ReportReason::Heartbeat)
        } else {
            None
        };
        if reason.is_some() {
            self.last = Some(ReportedProgress::of(progress, now_ms));
            self.unsent = false;
        }
        reason
    }

    /// What to tell the server about `progress`. A stopped Player names
    /// nothing, but the Track and playhead stay ready for the next device.
    fn report_of(&self, progress: &LocalProgress) -> LocalProgress {
        match &self.loaded {
            Some(loaded) if progress.source_key.is_none() => LocalProgress {
                playing: false,
                ..loaded.clone()
            },
            _ => progress.clone(),
        }
    }
}

impl ReportedProgress {
    fn of(progress: &LocalProgress, now_ms: u64) -> Self {
        Self {
            playing: progress.playing,
            position_seconds: progress.position_seconds,
            source_key: progress.source_key.clone(),
            reported_at_ms: now_ms,
        }
    }

    /// Where the playhead should be now, given what was reported.
    fn expected_seconds(&self, now_ms: u64) -> f64 {
        if !self.playing {
            return self.position_seconds;
        }
        self.position_seconds + (now_ms.saturating_sub(self.reported_at_ms) as f64) / 1000.0
    }
}

/// The playhead a snapshot describes now. Both times are the server's, so no
/// clock offset is needed.
pub(crate) fn expected_playhead_ms(snapshot: &SharedPlaybackSnapshot) -> u64 {
    if !snapshot.playing {
        return snapshot.position_ms;
    }
    snapshot.position_ms.saturating_add(
        snapshot
            .server_time_ms
            .saturating_sub(snapshot.position_at_ms),
    )
}

/// What the core does to the local Player to obey one snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ObeyAction {
    Pause,
    SeekTo(f64),
    Next,
    Previous,
    /// Another device became the Active one: silence the local Player.
    StopAndFollow,
}

/// What has been obeyed, so each command and skip acts once.
#[derive(Default)]
pub(crate) struct ObeyState {
    /// The latest judged snapshot named a device other than this one.
    another_device_active: bool,
    last_seen_revision: Option<u64>,
    last_obeyed_command_revision: u64,
    carried_skip_revision: u64,
    is_active: bool,
    claims_in_flight: u32,
    active_before_claim: bool,
    /// The newest snapshot from another device while a claim is in flight.
    /// Only the claim's answer can say whether it came before or after it.
    held: Option<SharedPlaybackSnapshot>,
}

impl ObeyState {
    fn is_following(&self) -> bool {
        self.another_device_active && self.claims_in_flight == 0
    }

    /// A renderer started the Player and is claiming: the sound stays until
    /// the answer, and the first snapshot naming this device starts afresh.
    fn begin_claim(&mut self) {
        if self.claims_in_flight == 0 {
            self.active_before_claim = self.is_active;
        }
        self.claims_in_flight += 1;
        self.is_active = false;
    }

    /// Returns the held snapshot, to be taken in after the claim's answer.
    fn end_claim(&mut self, claimed: bool) -> Option<SharedPlaybackSnapshot> {
        self.claims_in_flight = self.claims_in_flight.saturating_sub(1);
        if self.claims_in_flight > 0 {
            return None;
        }
        if !claimed {
            self.is_active = self.active_before_claim;
            self.held = None;
        }
        self.held.take()
    }

    /// Decides what to do about one snapshot, or `None` when it is not one
    /// to act on or pass on. Pure; `local` is what the local Player is doing
    /// right now, and may be absent. While `suspended` (a Room) only the
    /// revisions are kept up with.
    pub(crate) fn obey(
        &mut self,
        snapshot: &SharedPlaybackSnapshot,
        local: Option<&LocalProgress>,
        device_id: &str,
        suspended: bool,
    ) -> Option<Vec<ObeyAction>> {
        if self
            .last_seen_revision
            .is_some_and(|seen| snapshot.revision <= seen)
        {
            return None;
        }
        let is_active = snapshot.active_device_id.as_deref() == Some(device_id);
        if self.claims_in_flight > 0 && !is_active {
            if self
                .held
                .as_ref()
                .is_none_or(|held| held.revision < snapshot.revision)
            {
                self.held = Some(snapshot.clone());
            }
            return None;
        }
        self.last_seen_revision = Some(snapshot.revision);
        self.another_device_active = snapshot.active_device_id.is_some() && !is_active;
        let was_active = std::mem::replace(&mut self.is_active, is_active);
        let has_new_command = snapshot.remote_command_revision > self.last_obeyed_command_revision;
        let new_skip = snapshot
            .skip_request
            .as_ref()
            .filter(|skip| skip.revision > self.carried_skip_revision);
        let skip_direction = new_skip.map(|skip| skip.direction);
        self.last_obeyed_command_revision = self
            .last_obeyed_command_revision
            .max(snapshot.remote_command_revision);
        if let Some(skip) = new_skip {
            self.carried_skip_revision = skip.revision;
        }
        if suspended {
            return Some(Vec::new());
        }
        if !is_active {
            // A Player can also be sounding here without having been Active:
            // one output is the rule, however it started.
            let silence = self.another_device_active
                && (was_active || local.is_some_and(|local| local.playing));
            return Some(if silence {
                vec![ObeyAction::StopAndFollow]
            } else {
                Vec::new()
            });
        }
        // Commands from before this device was Active are history, as is a
        // skip left pending for a device that has since restarted.
        if !was_active {
            return Some(Vec::new());
        }
        let Some(local) = local.filter(|local| local.source_key.is_some()) else {
            return Some(Vec::new());
        };
        let mut actions = Vec::new();
        // The server coalesces frames, so one snapshot may carry a skip and a
        // pause together. Moving on makes the old playhead meaningless.
        match skip_direction {
            Some(SkipDirection::Next) => actions.push(ObeyAction::Next),
            Some(SkipDirection::Previous) => actions.push(ObeyAction::Previous),
            None => {}
        }
        if has_new_command {
            if !snapshot.playing && local.playing {
                actions.push(ObeyAction::Pause);
            }
            let local_ms = (local.position_seconds * 1000.0).round() as i64;
            let expected_ms = expected_playhead_ms(snapshot) as i64;
            if skip_direction.is_none()
                && (local_ms - expected_ms).abs() > (PLAYHEAD_JUMP_SECONDS * 1000.0) as i64
            {
                actions.push(ObeyAction::SeekTo(expected_ms as f64 / 1000.0));
            }
        }
        Some(actions)
    }
}

/// What the capability check last answered for one origin. An unanswered
/// check is retried after a heartbeat, so an unreachable server is not
/// hammered.
#[derive(Default)]
struct Gate {
    origin: Option<String>,
    capable: Option<bool>,
    retry_at_ms: u64,
}

/// Counts as an obeyed action in progress for as long as it lives.
struct Obeying(Arc<AtomicUsize>);

impl Obeying {
    fn begin(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self(count.clone())
    }
}

impl Drop for Obeying {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct SharedPlaybackServiceParts {
    pub(crate) bridge: Arc<HttpBridge>,
    pub(crate) origin: Arc<RwLock<Option<ServerOrigin>>>,
    pub(crate) output: Arc<Output>,
    pub(crate) playback: Weak<PlaybackController>,
    pub(crate) device: SharedPlaybackDevice,
    /// The receiver of the local Player states the controller publishes.
    /// Dropping its sender ends the service loop.
    pub(crate) local_states: watch::Receiver<Option<LocalProgress>>,
}

/// Reports the local Player and obeys remote commands, for as long as the
/// server advertises the capability. Dropping the service ends its loop.
#[derive(Clone)]
pub(crate) struct SharedPlaybackService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    bridge: Arc<HttpBridge>,
    origin: Arc<RwLock<Option<ServerOrigin>>>,
    device: SharedPlaybackDevice,
    output: Arc<Output>,
    playback: Weak<PlaybackController>,
    local_states: watch::Receiver<Option<LocalProgress>>,
    reporter: Mutex<ReporterState>,
    obeys: Mutex<ObeyState>,
    gate: Mutex<Gate>,
    /// A Room neither reports nor follows (ADR 0040).
    suspended: AtomicBool,
    /// Obeyed actions still reaching the Player: until they do, its state is
    /// the one the command replaced, and reporting it would undo the command.
    obeying: Arc<AtomicUsize>,
    /// Logged once per failure streak, not on every heartbeat.
    reporting_failed: AtomicBool,
}

impl SharedPlaybackService {
    pub(crate) fn start(parts: SharedPlaybackServiceParts) -> Self {
        let SharedPlaybackServiceParts {
            bridge,
            origin,
            output,
            playback,
            device,
            local_states,
        } = parts;
        let inner = Arc::new(ServiceInner {
            bridge,
            origin,
            device,
            output,
            playback,
            local_states: local_states.clone(),
            reporter: Mutex::default(),
            obeys: Mutex::default(),
            gate: Mutex::default(),
            suspended: AtomicBool::new(false),
            obeying: Arc::default(),
            reporting_failed: AtomicBool::new(false),
        });
        crate::runtime::spawn(run_service(Arc::clone(&inner), local_states));
        Self { inner }
    }

    pub(crate) fn device(&self) -> &SharedPlaybackDevice {
        &self.inner.device
    }

    pub(crate) fn set_suspended(&self, suspended: bool) {
        let was_suspended = self.inner.suspended.swap(suspended, Ordering::AcqRel);
        if was_suspended && !suspended {
            // What is loaded now is the Room's Track, not the user's.
            *self.inner.reporter() = ReporterState::default();
        }
    }

    /// Another device makes the sound, so OS controls here must not start any
    /// (ADR 0040). A claim in flight means this device is taking over.
    pub(crate) fn is_following(&self) -> bool {
        !self.inner.suspended.load(Ordering::Acquire) && self.inner.obeys().is_following()
    }

    /// Revisions belong to one Music Server; the next one counts its own.
    pub(crate) fn forget_server(&self) {
        *self.inner.obeys() = ObeyState::default();
        *self.inner.reporter() = ReporterState::default();
    }

    /// The server's answer now: a renderer works out its clock offset from
    /// it, so a remembered snapshot will not do. Without a server or
    /// capability, `None`: against an older server there is nothing to read.
    pub(crate) async fn current_state(
        &self,
    ) -> Result<Option<SharedPlaybackSnapshot>, ConnectionError> {
        if !self.inner.ensure_capable().await.unwrap_or(false) {
            return Ok(None);
        }
        let origin = self.inner.current_origin()?;
        let response = self
            .inner
            .bridge
            .send(
                &origin,
                HttpRequest {
                    method: "GET".to_owned(),
                    url: STATE_PATH.to_owned(),
                    headers: BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]),
                    body: None,
                },
            )
            .await?;
        if response.status == reqwest::StatusCode::NOT_FOUND.as_u16() {
            return Ok(None);
        }
        if !(200..300).contains(&response.status) {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                format!(
                    "Music Server returned HTTP {} for the Shared Playback State.",
                    response.status
                ),
            ));
        }
        let snapshot = parse_snapshot(&response.body)?;
        self.inner.on_snapshot(&snapshot);
        Ok(Some(snapshot))
    }

    /// Sends a command the renderer asked for, answering with the state the
    /// server returned. A conflict for a device that is no longer Active
    /// resolves with the state inside it. The shell has checked its shape.
    pub(crate) async fn send_command(
        &self,
        command: Value,
    ) -> Result<SharedPlaybackSnapshot, ConnectionError> {
        if !self.inner.ensure_capable().await? {
            return Err(ConnectionError::new(
                ConnectionErrorCode::CapabilityMismatch,
                format!(
                    "Music Server does not advertise {SHARED_SESSION_CAPABILITY}; Shared Playback is unavailable."
                ),
            ));
        }
        let is_claim = command.get("type").and_then(Value::as_str) == Some("claim");
        if is_claim {
            self.inner.obeys().begin_claim();
        }
        let answer = self.inner.post_command(&command).await;
        let held = is_claim
            .then(|| self.inner.obeys().end_claim(answer.is_ok()))
            .flatten();
        let snapshot = answer?;
        self.inner.on_snapshot(&snapshot);
        if let Some(held) = held {
            self.inner.on_snapshot(&held);
        }
        Ok(snapshot)
    }

    /// Feeds one snapshot from the event stream through obeying and on to
    /// the renderers. Such a frame also proves the capability, which is how
    /// a Music Server upgraded while this client runs is noticed.
    pub(crate) fn on_server_snapshot(&self, snapshot: SharedPlaybackSnapshot) {
        self.inner.open_gate();
        self.inner.on_snapshot(&snapshot);
    }

    /// The best-effort final report on app quit: the last playhead, paused.
    /// Everything it waits on is bounded, a silent server included.
    pub(crate) async fn shutdown_report(&self) {
        let Some(progress) = self.inner.local_progress() else {
            return;
        };
        if self.inner.suspended.load(Ordering::Acquire) || !self.inner.obeys().is_active {
            return;
        }
        let report = LocalProgress {
            playing: false,
            ..self.inner.reporter().report_of(&progress)
        };
        if report.source_key.is_none() {
            return;
        }
        let _ = tokio::time::timeout(SHUTDOWN_REPORT_TIMEOUT, async {
            if self.inner.ensure_capable().await.unwrap_or(false) {
                self.inner.post_report(&report).await;
            }
        })
        .await;
    }
}

impl ServiceInner {
    fn obeys(&self) -> MutexGuard<'_, ObeyState> {
        self.obeys.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reporter(&self) -> MutexGuard<'_, ReporterState> {
        self.reporter.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn local_progress(&self) -> Option<LocalProgress> {
        self.local_states.borrow().clone()
    }

    fn current_origin(&self) -> Result<ServerOrigin, ConnectionError> {
        self.origin
            .read()
            .ok()
            .and_then(|origin| origin.clone())
            .ok_or_else(|| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidOrigin,
                    "Configure a Music Server before using Shared Playback.",
                )
            })
    }

    fn gate(&self) -> MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open_gate(&self) {
        if let Ok(origin) = self.current_origin() {
            *self.gate() = Gate {
                origin: Some(origin.as_str().to_owned()),
                capable: Some(true),
                retry_at_ms: 0,
            };
        }
    }

    /// Whether the current server advertises the capability, cached per
    /// origin; a check without an answer is retried after a heartbeat.
    async fn ensure_capable(&self) -> Result<bool, ConnectionError> {
        let origin = self.current_origin()?;
        let origin_key = origin.as_str().to_owned();
        let now_ms = monotonic_ms();
        {
            let gate = self.gate();
            if gate.origin.as_deref() == Some(origin_key.as_str()) {
                if let Some(capable) = gate.capable {
                    return Ok(capable);
                }
                if now_ms < gate.retry_at_ms {
                    return Err(ConnectionError::new(
                        ConnectionErrorCode::Unreachable,
                        "Music Server did not answer the Shared Playback capability check.",
                    ));
                }
            }
        }
        let check = self.bridge.test_server(&origin).await;
        let mut gate = self.gate();
        gate.origin = Some(origin_key);
        match check {
            Ok(checked) => {
                let capable = checked
                    .capabilities
                    .iter()
                    .any(|capability| capability == SHARED_SESSION_CAPABILITY);
                gate.capable = Some(capable);
                Ok(capable)
            }
            Err(error) => {
                gate.capable = None;
                gate.retry_at_ms = now_ms + HEARTBEAT_MS;
                Err(error)
            }
        }
    }

    async fn post_command(&self, body: &Value) -> Result<SharedPlaybackSnapshot, ConnectionError> {
        let origin = self.current_origin()?;
        let response = self
            .bridge
            .send(
                &origin,
                HttpRequest {
                    method: "POST".to_owned(),
                    url: STATE_COMMANDS_PATH.to_owned(),
                    headers: BTreeMap::from([
                        ("accept".to_owned(), "application/json".to_owned()),
                        ("content-type".to_owned(), "application/json".to_owned()),
                    ]),
                    body: Some(serde_json::to_vec(body).map_err(|error| {
                        ConnectionError::new(
                            ConnectionErrorCode::InvalidRequest,
                            format!("Shared Playback command could not be encoded: {error}"),
                        )
                    })?),
                },
            )
            .await?;
        if response.status == reqwest::StatusCode::CONFLICT.as_u16() {
            // The sender is no longer the Active Device; the body carries the
            // truth it must accept instead of its own.
            return parse_conflict(&response.body);
        }
        if !(200..300).contains(&response.status) {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                format!(
                    "Music Server returned HTTP {} for a Shared Playback command.",
                    response.status
                ),
            ));
        }
        parse_snapshot(&response.body)
    }

    fn on_snapshot(&self, snapshot: &SharedPlaybackSnapshot) {
        let local = self.local_progress();
        let suspended = self.suspended.load(Ordering::Acquire);
        let Some(actions) = self
            .obeys()
            .obey(snapshot, local.as_ref(), &self.device.id, suspended)
        else {
            return;
        };
        self.output.emit(SHARED_PLAYBACK_STATE_EVENT, snapshot);
        self.execute(actions);
    }

    fn execute(&self, actions: Vec<ObeyAction>) {
        if actions.is_empty() {
            return;
        }
        let Some(controller) = self.playback.upgrade() else {
            return;
        };
        let obeying = Obeying::begin(&self.obeying);
        crate::runtime::spawn_blocking(move || {
            let _obeying = obeying;
            for action in actions {
                let result = match action {
                    ObeyAction::Pause => controller.pause().map(|_| ()),
                    ObeyAction::SeekTo(seconds) => controller.seek(seconds).map(|_| ()),
                    ObeyAction::Next => controller.next().map(|_| ()),
                    ObeyAction::Previous => controller.previous().map(|_| ()),
                    ObeyAction::StopAndFollow => controller.stop().map(|_| ()),
                };
                if let Err(error) = result {
                    eprintln!(
                        "Shared Playback command could not be obeyed: {}",
                        error.message
                    );
                }
            }
        });
    }

    /// Reports the local Player when a trigger fires. Only the Active Device
    /// reports, and one report at a time, so they cannot land out of order.
    async fn report_if_due(&self) {
        if self.suspended.load(Ordering::Acquire) || self.obeying.load(Ordering::Acquire) > 0 {
            return;
        }
        if !self.obeys().is_active {
            *self.reporter() = ReporterState::default();
            return;
        }
        let Some(progress) = self.local_progress() else {
            return;
        };
        let report = {
            let mut reporter = self.reporter();
            reporter
                .next_report(&progress, monotonic_ms())
                .map(|_| reporter.report_of(&progress))
        };
        let Some(report) = report else {
            return;
        };
        match self.ensure_capable().await {
            Ok(true) => self.post_report(&report).await,
            // Against an older server do nothing.
            Ok(false) => {}
            Err(error) => self.report_failed(&error),
        }
    }

    async fn post_report(&self, progress: &LocalProgress) {
        let based_on_revision = self.obeys().last_seen_revision.unwrap_or(0);
        let position_ms = (progress.position_seconds.max(0.0) * 1000.0).round() as u64;
        let command = json!({
            "type": "report",
            "deviceId": self.device.id,
            "basedOnRevision": based_on_revision,
            "queueItemId": progress.queue_item_id,
            "trackId": progress.track_id,
            "positionMs": position_ms,
            "playing": progress.playing,
        });
        match self.post_command(&command).await {
            Ok(snapshot) => {
                self.reporting_failed.store(false, Ordering::Release);
                self.on_snapshot(&snapshot);
            }
            Err(error) => self.report_failed(&error),
        }
    }

    fn report_failed(&self, error: &ConnectionError) {
        self.reporter().unsent = true;
        if !self.reporting_failed.swap(true, Ordering::AcqRel) {
            eprintln!("Shared Playback report failed: {}", error.message);
        }
    }
}

fn parse_snapshot(body: &[u8]) -> Result<SharedPlaybackSnapshot, ConnectionError> {
    serde_json::from_slice(body).map_err(|_| {
        ConnectionError::new(
            ConnectionErrorCode::InvalidResponse,
            "Music Server Shared Playback State is not valid.",
        )
    })
}

#[derive(Deserialize)]
struct StateConflict {
    code: String,
    state: SharedPlaybackSnapshot,
}

/// Reads the state inside a 409 that says this device is no longer Active.
fn parse_conflict(body: &[u8]) -> Result<SharedPlaybackSnapshot, ConnectionError> {
    serde_json::from_slice::<StateConflict>(body)
        .ok()
        .filter(|conflict| conflict.code == "playback_device_not_active")
        .map(|conflict| conflict.state)
        .ok_or_else(|| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Music Server refused a Shared Playback command without a known conflict.",
            )
        })
}

/// Milliseconds on the process's monotonic clock, for report triggers.
fn monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

async fn run_service(
    inner: Arc<ServiceInner>,
    mut local_states: watch::Receiver<Option<LocalProgress>>,
) {
    let mut heartbeat = tokio::time::interval(HEARTBEAT_TICK);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = local_states.changed() => {
                if changed.is_err() {
                    return;
                }
                local_states.borrow_and_update();
            }
            _ = heartbeat.tick() => {}
        }
        inner.report_if_due().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HEARTBEAT_MS, LocalProgress, ObeyAction, ObeyState, Obeying, ReportReason, ReporterState,
        SHARED_SESSION_CAPABILITY, SharedPlaybackDevice, SharedPlaybackDeviceStore,
        SharedPlaybackService, SharedPlaybackSnapshot, SkipDirection, SkipRequest,
        expected_playhead_ms, local_progress,
    };
    use crate::connection::{ConnectionErrorCode, HttpBridge, ServerOrigin};
    use crate::playback::{PlaybackSessionState, PlaybackStatus};
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, RwLock, Weak};
    use std::time::{Duration, Instant};

    const DEVICE: &str = "device-1";
    const WITH_SHARED_SESSION: &[&str] = &[
        "api.v1",
        "playback.queue-events.v1",
        "managed-import-batches.v1",
        "managed-track-deletion.v1",
        "managed-track-replacement.v1",
        SHARED_SESSION_CAPABILITY,
    ];
    const WITHOUT_SHARED_SESSION: &[&str] = &[
        "api.v1",
        "playback.queue-events.v1",
        "managed-import-batches.v1",
        "managed-track-deletion.v1",
        "managed-track-replacement.v1",
    ];

    fn progress(playing: bool, position_seconds: f64, source_key: &str) -> LocalProgress {
        LocalProgress {
            playing,
            position_seconds,
            source_key: Some(source_key.to_owned()),
            queue_item_id: Some("item-1".to_owned()),
            track_id: Some("track-1".to_owned()),
        }
    }

    fn idle() -> LocalProgress {
        LocalProgress {
            playing: false,
            position_seconds: 0.0,
            source_key: None,
            queue_item_id: None,
            track_id: None,
        }
    }

    fn snapshot(revision: u64, active_device_id: Option<&str>) -> SharedPlaybackSnapshot {
        SharedPlaybackSnapshot {
            revision,
            queue_item_id: Some("item-1".to_owned()),
            track_id: Some("track-1".to_owned()),
            playing: true,
            position_ms: 30_000,
            position_at_ms: 1_000,
            active_device_id: active_device_id.map(str::to_owned),
            active_device_name: None,
            remote_command_revision: 0,
            skip_request: None,
            server_time_ms: 1_000,
        }
    }

    fn command(revision: u64) -> SharedPlaybackSnapshot {
        SharedPlaybackSnapshot {
            remote_command_revision: revision,
            ..snapshot(revision, Some(DEVICE))
        }
    }

    fn with_skip(revision: u64, direction: SkipDirection) -> SharedPlaybackSnapshot {
        SharedPlaybackSnapshot {
            skip_request: Some(SkipRequest {
                revision,
                direction,
            }),
            ..command(revision)
        }
    }

    /// An `ObeyState` that already knows this device is the Active one.
    fn active() -> ObeyState {
        let mut obeys = ObeyState::default();
        assert_eq!(
            obeys.obey(&snapshot(1, Some(DEVICE)), None, DEVICE, false),
            Some(Vec::new())
        );
        obeys
    }

    fn obey(obeys: &mut ObeyState, state: &SharedPlaybackSnapshot) -> Option<Vec<ObeyAction>> {
        obeys.obey(
            state,
            Some(&progress(true, 30.0, "track:t1:")),
            DEVICE,
            false,
        )
    }

    /// A reporter past its silent first look at the Player.
    fn reporter_at(state: &LocalProgress, now_ms: u64) -> ReporterState {
        let mut reporter = ReporterState::default();
        assert_eq!(reporter.next_report(state, now_ms), None);
        reporter
    }

    #[test]
    fn a_player_with_nothing_loaded_reports_nothing() {
        let mut reporter = ReporterState::default();

        assert_eq!(reporter.next_report(&idle(), 0), None);
        assert_eq!(reporter.next_report(&idle(), HEARTBEAT_MS * 10), None);
    }

    #[test]
    fn the_first_look_at_the_player_says_nothing() {
        // A restored playhead must not overwrite the shared one.
        let mut reporter = ReporterState::default();

        assert_eq!(
            reporter.next_report(&progress(false, 12.0, "track:t1:"), 100),
            None
        );
        assert_eq!(
            reporter.next_report(&progress(false, 12.0, "track:t1:"), 200),
            None
        );
    }

    #[test]
    fn play_pause_and_source_changes_report_immediately() {
        let mut reporter = reporter_at(&progress(true, 10.0, "track:t1:"), 1_000);
        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), 1_100),
            Some(ReportReason::PlayPause)
        );
        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), 1_200),
            None
        );
        assert_eq!(
            reporter.next_report(&progress(false, 30.0, "track:t2:"), 1_300),
            Some(ReportReason::SourceChange)
        );
    }

    #[test]
    fn a_stopped_player_keeps_the_track_and_playhead_for_the_next_device() {
        let mut reporter = reporter_at(&progress(true, 10.0, "track:t1:"), 1_000);
        assert_eq!(
            reporter.next_report(&progress(true, 11.0, "track:t1:"), 2_000),
            None
        );

        assert_eq!(
            reporter.next_report(&idle(), 2_100),
            Some(ReportReason::PlayPause)
        );
        assert_eq!(
            reporter.report_of(&idle()),
            progress(false, 11.0, "track:t1:")
        );
    }

    #[test]
    fn a_playhead_jump_beyond_one_and_a_half_seconds_reports_at_once() {
        let mut reporter = reporter_at(&progress(true, 10.0, "track:t1:"), 1_000);
        // 1.4 s ahead of the expected 11.0 s is no jump.
        assert_eq!(
            reporter.next_report(&progress(true, 12.4, "track:t1:"), 2_000),
            None
        );
        // 1.6 s behind the expected 11.0 s is a seek to report.
        assert_eq!(
            reporter.next_report(&progress(true, 9.4, "track:t1:"), 2_000),
            Some(ReportReason::PlayheadJump)
        );
        // And the jump becomes the new expected playhead.
        assert_eq!(
            reporter.next_report(&progress(true, 9.5, "track:t1:"), 2_100),
            None
        );
    }

    #[test]
    fn a_paused_seek_still_reports_when_it_moves_the_playhead() {
        let mut reporter = reporter_at(&progress(false, 10.0, "track:t1:"), 1_000);

        assert_eq!(
            reporter.next_report(&progress(false, 40.0, "track:t1:"), 5_000),
            Some(ReportReason::PlayheadJump)
        );
    }

    #[test]
    fn the_heartbeat_reports_every_five_seconds_while_playing() {
        let mut reporter = reporter_at(&progress(true, 0.0, "track:t1:"), 0);
        assert_eq!(
            reporter.next_report(&progress(true, 4.0, "track:t1:"), HEARTBEAT_MS - 1),
            None
        );
        assert_eq!(
            reporter.next_report(&progress(true, 5.0, "track:t1:"), HEARTBEAT_MS),
            Some(ReportReason::Heartbeat)
        );
        assert_eq!(
            reporter.next_report(&progress(true, 9.0, "track:t1:"), HEARTBEAT_MS * 2 - 1),
            None
        );
        assert_eq!(
            reporter.next_report(&progress(true, 10.0, "track:t1:"), HEARTBEAT_MS * 2),
            Some(ReportReason::Heartbeat)
        );
    }

    #[test]
    fn a_paused_player_needs_no_heartbeat() {
        let mut reporter = reporter_at(&progress(false, 10.0, "track:t1:"), 0);

        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), HEARTBEAT_MS * 10),
            None
        );
    }

    #[test]
    fn a_report_that_failed_is_tried_again_on_the_heartbeat_only() {
        let mut reporter = reporter_at(&progress(true, 10.0, "track:t1:"), 0);
        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), 100),
            Some(ReportReason::PlayPause)
        );
        reporter.unsent = true;

        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), HEARTBEAT_MS),
            None
        );
        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), HEARTBEAT_MS + 100),
            Some(ReportReason::Heartbeat)
        );
        assert_eq!(
            reporter.next_report(&progress(false, 10.0, "track:t1:"), HEARTBEAT_MS * 3),
            None
        );
    }

    #[test]
    fn local_playback_state_distills_playing_position_and_track_ids() {
        let mut state = PlaybackSessionState::default();
        state.status = PlaybackStatus::Playing;
        state.current_time = 42.5;
        state.source = Some(json!({
            "type": "track",
            "track": { "id": "track-9" },
            "queueItemId": "item-7",
        }));

        let progress = local_progress(&state);
        assert!(progress.playing);
        assert_eq!(progress.position_seconds, 42.5);
        assert_eq!(progress.track_id.as_deref(), Some("track-9"));
        assert_eq!(progress.queue_item_id.as_deref(), Some("item-7"));
    }

    #[test]
    fn non_track_sources_report_null_ids() {
        let mut state = PlaybackSessionState::default();
        state.status = PlaybackStatus::Playing;
        state.source = Some(json!({
            "type": "radio-station",
            "station": { "id": "station-3" },
        }));

        let progress = local_progress(&state);
        assert_eq!(progress.track_id, None);
        assert_eq!(progress.queue_item_id, None);
        assert_eq!(
            progress.source_key.as_deref(),
            Some("radio-station:station-3:")
        );
    }

    #[test]
    fn a_paused_snapshot_expects_its_reported_playhead() {
        let mut state = snapshot(1, Some(DEVICE));
        state.playing = false;
        state.position_ms = 42_000;
        state.server_time_ms = 90_000;

        assert_eq!(expected_playhead_ms(&state), 42_000);
    }

    #[test]
    fn a_playing_snapshot_advances_from_its_server_timestamp() {
        let mut state = snapshot(1, Some(DEVICE));
        state.position_ms = 42_000;
        state.position_at_ms = 90_000;
        state.server_time_ms = 92_500;

        assert_eq!(expected_playhead_ms(&state), 44_500);
    }

    #[test]
    fn the_active_device_ignores_its_own_reports() {
        let mut obeys = active();

        assert_eq!(obey(&mut obeys, &snapshot(2, Some(DEVICE))), Some(vec![]));
    }

    #[test]
    fn commands_from_before_the_first_snapshot_are_history() {
        // A restart must not replay a pause or a skip left for this device.
        let mut obeys = ObeyState::default();
        let mut old = with_skip(9, SkipDirection::Next);
        old.playing = false;

        assert_eq!(obey(&mut obeys, &old), Some(vec![]));
        assert_eq!(obey(&mut obeys, &snapshot(10, Some(DEVICE))), Some(vec![]));
    }

    #[test]
    fn a_remote_pause_pauses_the_playing_active_device_once() {
        let mut obeys = active();
        let mut paused = command(2);
        paused.playing = false;

        assert_eq!(obey(&mut obeys, &paused), Some(vec![ObeyAction::Pause]));
        assert_eq!(obey(&mut obeys, &paused), None);
    }

    #[test]
    fn a_remote_pause_also_moves_a_drifted_playhead() {
        let mut obeys = active();
        let mut paused = command(2);
        paused.playing = false;
        paused.position_ms = 90_000;

        assert_eq!(
            obey(&mut obeys, &paused),
            Some(vec![ObeyAction::Pause, ObeyAction::SeekTo(90.0)])
        );
    }

    #[test]
    fn a_remote_pause_leaves_a_silent_player_alone() {
        let mut obeys = active();
        let mut paused = command(2);
        paused.playing = false;

        assert_eq!(
            obeys.obey(
                &paused,
                Some(&progress(false, 30.0, "track:t1:")),
                DEVICE,
                false
            ),
            Some(vec![])
        );
    }

    #[test]
    fn nothing_is_obeyed_with_nothing_loaded() {
        // A device that took over paused has not loaded its source yet.
        let mut obeys = active();

        assert_eq!(
            obeys.obey(
                &with_skip(2, SkipDirection::Next),
                Some(&idle()),
                DEVICE,
                false
            ),
            Some(vec![])
        );
    }

    #[test]
    fn a_remote_seek_moves_a_drifted_playhead() {
        let mut obeys = active();
        let mut sought = command(2);
        sought.position_ms = 120_000;
        sought.position_at_ms = 10_000;
        sought.server_time_ms = 11_000;

        assert_eq!(
            obey(&mut obeys, &sought),
            Some(vec![ObeyAction::SeekTo(121.0)])
        );
    }

    #[test]
    fn a_remote_seek_within_the_jump_limit_moves_nothing() {
        let mut obeys = active();
        let mut sought = command(2);
        sought.position_ms = 31_000;

        assert_eq!(obey(&mut obeys, &sought), Some(vec![]));
    }

    #[test]
    fn a_skip_is_carried_out_once_per_revision() {
        let mut obeys = active();

        assert_eq!(
            obey(&mut obeys, &with_skip(3, SkipDirection::Next)),
            Some(vec![ObeyAction::Next])
        );
        let mut skipped = snapshot(4, Some(DEVICE));
        skipped.skip_request = Some(SkipRequest {
            revision: 3,
            direction: SkipDirection::Next,
        });
        skipped.remote_command_revision = 3;
        assert_eq!(obey(&mut obeys, &skipped), Some(vec![]));

        assert_eq!(
            obey(&mut obeys, &with_skip(5, SkipDirection::Previous)),
            Some(vec![ObeyAction::Previous])
        );
    }

    #[test]
    fn a_skip_does_not_also_seek_to_the_old_track_playhead() {
        let mut obeys = active();
        let mut skip = with_skip(3, SkipDirection::Next);
        skip.position_ms = 120_000;

        assert_eq!(obey(&mut obeys, &skip), Some(vec![ObeyAction::Next]));
    }

    #[test]
    fn a_skip_and_a_pause_coalesced_into_one_snapshot_both_act() {
        let mut obeys = active();
        let mut both = with_skip(3, SkipDirection::Next);
        both.playing = false;

        assert_eq!(
            obey(&mut obeys, &both),
            Some(vec![ObeyAction::Next, ObeyAction::Pause])
        );
    }

    #[test]
    fn a_player_sounding_while_following_is_silenced() {
        let mut obeys = ObeyState::default();
        assert!(!obeys.is_following());
        let playing = progress(true, 1.0, "track-1");
        assert_eq!(
            obeys.obey(
                &snapshot(1, Some("device-2")),
                Some(&playing),
                DEVICE,
                false
            ),
            Some(vec![ObeyAction::StopAndFollow])
        );
        assert!(obeys.is_following());
        obeys.begin_claim();
        assert!(!obeys.is_following());
    }

    #[test]
    fn another_active_device_silences_the_previous_one() {
        let mut obeys = active();

        assert_eq!(
            obey(&mut obeys, &snapshot(2, Some("device-2"))),
            Some(vec![ObeyAction::StopAndFollow])
        );
        // Edge-triggered: the same device reporting on stops nothing more.
        assert_eq!(
            obeys.obey(&snapshot(3, Some("device-2")), None, DEVICE, false),
            Some(vec![])
        );
    }

    #[test]
    fn following_snapshots_obey_nothing_and_cannot_be_replayed_after_a_takeover() {
        let mut obeys = ObeyState::default();
        let mut remote = with_skip(7, SkipDirection::Next);
        remote.active_device_id = Some("device-2".to_owned());
        remote.playing = false;
        remote.position_ms = 90_000;
        assert_eq!(obeys.obey(&remote, None, DEVICE, false), Some(vec![]));

        assert_eq!(obey(&mut obeys, &snapshot(8, Some(DEVICE))), Some(vec![]));
        assert_eq!(obey(&mut obeys, &snapshot(9, Some(DEVICE))), Some(vec![]));
    }

    #[test]
    fn snapshots_older_than_the_last_seen_are_ignored() {
        let mut obeys = active();
        assert_eq!(obey(&mut obeys, &snapshot(5, Some(DEVICE))), Some(vec![]));

        assert_eq!(obey(&mut obeys, &snapshot(4, Some("device-2"))), None);
    }

    #[test]
    fn a_snapshot_older_than_a_takeover_claim_stops_nothing() {
        let mut obeys = active();
        obeys.begin_claim();

        // Another device's report, from before the claim landed.
        assert_eq!(obey(&mut obeys, &snapshot(2, Some("device-2"))), None);

        let held = obeys.end_claim(true).expect("the held snapshot");
        assert_eq!(obey(&mut obeys, &snapshot(3, Some(DEVICE))), Some(vec![]));
        assert_eq!(obey(&mut obeys, &held), None);
        assert!(obeys.is_active);
    }

    #[test]
    fn a_snapshot_newer_than_a_takeover_claim_still_stops_the_player() {
        let mut obeys = active();
        obeys.begin_claim();
        assert_eq!(obey(&mut obeys, &snapshot(4, Some("device-2"))), None);

        let held = obeys.end_claim(true).expect("the held snapshot");
        assert_eq!(obey(&mut obeys, &snapshot(3, Some(DEVICE))), Some(vec![]));
        assert_eq!(
            obey(&mut obeys, &held),
            Some(vec![ObeyAction::StopAndFollow])
        );
    }

    #[test]
    fn a_claim_seen_on_the_stream_before_its_answer_leaves_the_device_active() {
        let mut obeys = active();
        obeys.begin_claim();

        assert_eq!(obey(&mut obeys, &snapshot(3, Some(DEVICE))), Some(vec![]));
        assert_eq!(obeys.end_claim(true), None);
        assert_eq!(obey(&mut obeys, &snapshot(3, Some(DEVICE))), None);

        assert!(obeys.is_active);
    }

    #[test]
    fn a_claim_answer_does_not_replay_a_command_the_claim_overtook() {
        let mut obeys = active();
        obeys.begin_claim();
        assert_eq!(obeys.end_claim(true), None);
        let mut answer = command(3);
        answer.position_ms = 90_000;

        assert_eq!(obey(&mut obeys, &answer), Some(vec![]));
    }

    #[test]
    fn a_failed_claim_keeps_what_the_device_was() {
        let mut obeys = active();
        obeys.begin_claim();
        assert_eq!(obey(&mut obeys, &snapshot(2, Some("device-2"))), None);

        assert_eq!(obeys.end_claim(false), None);

        assert!(obeys.is_active);
    }

    #[test]
    fn a_room_neither_obeys_nor_stops_but_keeps_up_with_revisions() {
        let mut obeys = active();
        let mut paused = command(2);
        paused.playing = false;
        assert_eq!(
            obeys.obey(
                &paused,
                Some(&progress(true, 30.0, "track:t1:")),
                DEVICE,
                true
            ),
            Some(vec![])
        );
        assert_eq!(
            obeys.obey(&snapshot(3, Some("device-2")), None, DEVICE, true),
            Some(vec![])
        );

        // Leaving the Room does not replay the pause it sat out.
        assert_eq!(obey(&mut obeys, &snapshot(4, Some(DEVICE))), Some(vec![]));
    }

    #[test]
    fn the_device_name_is_harmony_on_the_host() {
        let device = SharedPlaybackDevice::new(
            "01971a52-1d3f-7cc1-9a4f-6f9db4a5d0e1".to_owned(),
            "music-desk",
        );
        assert_eq!(device.name, "Harmony on music-desk");

        let long: String = "x".repeat(100);
        let device = SharedPlaybackDevice::new("id".to_owned(), &long);
        assert_eq!(device.name.chars().count(), 64);
        assert!(device.name.starts_with("Harmony on "));
    }

    #[test]
    fn the_device_identity_is_created_once_and_kept() {
        let directory =
            std::env::temp_dir().join(format!("shared-playback-device-{}", uuid::Uuid::new_v4()));
        let store = SharedPlaybackDeviceStore::new(directory.join("shared-playback-device.json"));

        let first = store.load_or_create();
        let second = SharedPlaybackDeviceStore::new(
            PathBuf::from(&directory).join("shared-playback-device.json"),
        )
        .load_or_create();

        assert_eq!(first, second);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_broken_device_record_reads_as_a_new_device() {
        let directory = std::env::temp_dir().join(format!(
            "shared-playback-device-broken-{}",
            uuid::Uuid::new_v4()
        ));
        let path = directory.join("shared-playback-device.json");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&path, "not json").unwrap();

        let device = SharedPlaybackDeviceStore::new(path).load_or_create();

        assert!(uuid::Uuid::parse_str(&device.id).is_ok());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_device_that_cannot_be_saved_still_starts() {
        let file = std::env::temp_dir().join(format!(
            "shared-playback-device-file-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&file, "a file where a directory should be").unwrap();

        let device = SharedPlaybackDeviceStore::new(file.join("device.json")).load_or_create();

        assert!(uuid::Uuid::parse_str(&device.id).is_ok());
        std::fs::remove_file(file).unwrap();
    }

    type Recorded = Arc<Mutex<Vec<Value>>>;

    /// A Music Server that answers health with `capabilities`, records every
    /// Shared Playback command, and answers each with one state, wrapped as a
    /// conflict for a 409.
    async fn serve_shared_playback(
        capabilities: &[&str],
        command_status: &'static str,
    ) -> (ServerOrigin, Recorded) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Shared Playback server");
        let origin = ServerOrigin::parse(&format!(
            "http://{}",
            listener.local_addr().expect("address")
        ))
        .expect("origin");
        let recorded = Recorded::default();
        let recorded_tasks = recorded.clone();
        let capabilities: Vec<String> = capabilities
            .iter()
            .map(|capability| capability.to_string())
            .collect();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let recorded = recorded_tasks.clone();
                let capabilities = capabilities.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 8192];
                    let read = tokio::io::AsyncReadExt::read(&mut socket, &mut buffer)
                        .await
                        .expect("read request");
                    let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let state = json!({
                        "revision": 12,
                        "queueItemId": null,
                        "trackId": null,
                        "playing": false,
                        "positionMs": 0,
                        "positionAtMs": 1_000,
                        "activeDeviceId": "device-under-test",
                        "activeDeviceName": "Harmony on test",
                        "remoteCommandRevision": 0,
                        "skipRequest": null,
                        "serverTimeMs": 1_000
                    });
                    let (status, body) = if request.starts_with("GET /api/v1/health ") {
                        let health = json!({
                            "status": "ok",
                            "version": "0.1.0-test",
                            "capabilities": capabilities,
                        });
                        ("200 OK", health)
                    } else if request.starts_with("GET ") {
                        ("200 OK", state)
                    } else {
                        let body = request.find("\r\n\r\n").map(|at| &request[at + 4..]);
                        if let Some(Ok(command)) = body.map(serde_json::from_str::<Value>) {
                            recorded.lock().expect("recorded").push(command);
                        }
                        let answer = if command_status.starts_with("409") {
                            json!({ "code": "playback_device_not_active", "state": state })
                        } else {
                            state
                        };
                        (command_status, answer)
                    };
                    let body = body.to_string();
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    use tokio::io::AsyncWriteExt;
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("write response");
                });
            }
        });
        (origin, recorded)
    }

    struct Drain;

    impl std::io::Write for Drain {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn test_service(
        origin: ServerOrigin,
    ) -> (
        SharedPlaybackService,
        tokio::sync::watch::Sender<Option<LocalProgress>>,
    ) {
        let (local_sender, local_states) = tokio::sync::watch::channel(None);
        let service = SharedPlaybackService::start(super::SharedPlaybackServiceParts {
            bridge: Arc::new(HttpBridge::new().expect("bridge")),
            origin: Arc::new(RwLock::new(Some(origin))),
            output: Arc::new(crate::rpc::Output::new(Box::new(Drain))),
            playback: Weak::new(),
            device: SharedPlaybackDevice {
                id: "device-under-test".to_owned(),
                name: "Harmony on test".to_owned(),
            },
            local_states,
        });
        (service, local_sender)
    }

    /// Past the first silent look, so the next local change is a report.
    async fn playing_as_active_device(
        service: &SharedPlaybackService,
        local_sender: &tokio::sync::watch::Sender<Option<LocalProgress>>,
    ) {
        service.on_server_snapshot(snapshot(10, Some("device-under-test")));
        local_sender
            .send(Some(progress(true, 10.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    async fn recorded_reports(recorded: &Recorded, count: usize) -> Vec<Value> {
        for _ in 0..300 {
            let ready = {
                let reports = recorded.lock().expect("recorded");
                reports.len() >= count
            };
            if ready {
                return recorded.lock().expect("recorded").clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {count} reported commands");
    }

    #[tokio::test]
    async fn reports_fire_on_playback_and_carry_the_revision_they_saw() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        playing_as_active_device(&service, &local_sender).await;

        local_sender
            .send(Some(progress(false, 11.0, "track:t1:")))
            .expect("local state");

        let reports = recorded_reports(&recorded, 1).await;
        assert_eq!(reports[0]["type"], "report");
        assert_eq!(reports[0]["deviceId"], "device-under-test");
        assert_eq!(reports[0]["playing"], false);
        assert_eq!(reports[0]["positionMs"], 11_000);
        assert_eq!(reports[0]["basedOnRevision"], 10);
        assert_eq!(reports[0]["trackId"], "track-1");
        assert_eq!(reports[0]["queueItemId"], "item-1");
    }

    #[tokio::test]
    async fn a_device_that_is_not_active_reports_nothing() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        service.on_server_snapshot(snapshot(10, Some("another-device")));

        local_sender
            .send(Some(progress(true, 10.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(100)).await;
        local_sender
            .send(Some(progress(false, 10.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(recorded.lock().expect("recorded").is_empty());
    }

    #[tokio::test]
    async fn a_room_reports_nothing_and_the_final_report_stays_out_of_it_too() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        playing_as_active_device(&service, &local_sender).await;
        service.set_suspended(true);

        local_sender
            .send(Some(progress(false, 11.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(200)).await;
        service.shutdown_report().await;

        assert!(recorded.lock().expect("recorded").is_empty());
    }

    #[tokio::test]
    async fn no_report_goes_out_while_an_obeyed_command_reaches_the_player() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        playing_as_active_device(&service, &local_sender).await;
        let obeying = Obeying::begin(&service.inner.obeying);

        // Still playing at the revision of the pause: that report would be
        // accepted, and would undo the pause.
        local_sender
            .send(Some(progress(true, 20.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(recorded.lock().expect("recorded").is_empty());

        drop(obeying);
        local_sender
            .send(Some(progress(false, 20.0, "track:t1:")))
            .expect("local state");
        let reports = recorded_reports(&recorded, 1).await;
        assert_eq!(reports[0]["playing"], false);
    }

    #[tokio::test]
    async fn an_older_server_is_left_alone() {
        let (origin, recorded) = serve_shared_playback(WITHOUT_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);

        local_sender
            .send(Some(progress(true, 10.0, "track:t1:")))
            .expect("local state");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(service.current_state().await.expect("state"), None);
        let error = service
            .send_command(json!({ "type": "pause" }))
            .await
            .expect_err("no capability");
        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        service.shutdown_report().await;
        assert!(recorded.lock().expect("recorded").is_empty());
    }

    #[tokio::test]
    async fn a_stream_frame_proves_a_server_upgraded_since_the_last_check() {
        let (origin, recorded) = serve_shared_playback(WITHOUT_SHARED_SESSION, "200 OK").await;
        let (service, _local_sender) = test_service(origin);
        assert_eq!(service.current_state().await.expect("state"), None);

        service.on_server_snapshot(snapshot(3, Some("another-device")));

        service
            .send_command(json!({ "type": "pause" }))
            .await
            .expect("capability proved");
        assert_eq!(recorded.lock().expect("recorded").len(), 1);
    }

    #[tokio::test]
    async fn a_failed_capability_check_waits_a_heartbeat_before_the_next() {
        // Not a port freed by a listener here: a parallel test could get it.
        let origin = ServerOrigin::parse("http://127.0.0.1:1").expect("origin");
        let (service, _local_sender) = test_service(origin);

        let first = service.inner.ensure_capable().await.expect_err("refused");
        let second = service.inner.ensure_capable().await.expect_err("waiting");

        assert_ne!(first.message, second.message);
    }

    #[tokio::test]
    async fn the_renderer_reads_the_server_not_a_remembered_snapshot() {
        let (origin, _recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, _local_sender) = test_service(origin);
        service.on_server_snapshot(snapshot(40, Some("another-device")));

        let state = service
            .current_state()
            .await
            .expect("state")
            .expect("known");

        assert_eq!(state.revision, 12);
    }

    #[tokio::test]
    async fn the_next_server_counts_its_own_revisions() {
        let (origin, _recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, _local_sender) = test_service(origin);
        service.on_server_snapshot(snapshot(40, Some("device-under-test")));

        service.forget_server();
        service.on_server_snapshot(snapshot(2, Some("device-under-test")));

        assert_eq!(service.inner.obeys().last_seen_revision, Some(2));
    }

    #[tokio::test]
    async fn a_conflict_answer_resolves_with_the_state_inside_it() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "409 Conflict").await;
        let (service, _local_sender) = test_service(origin);

        let state = service
            .send_command(json!({ "type": "pause" }))
            .await
            .expect("conflict state");

        assert_eq!(state.revision, 12);
        assert_eq!(recorded.lock().expect("recorded")[0]["type"], "pause");
    }

    #[tokio::test]
    async fn the_final_report_is_paused_and_keeps_the_track() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        playing_as_active_device(&service, &local_sender).await;

        service.shutdown_report().await;

        let reports = recorded_reports(&recorded, 1).await;
        assert_eq!(reports[0]["playing"], false);
        assert_eq!(reports[0]["positionMs"], 10_000);
        assert_eq!(reports[0]["trackId"], "track-1");
    }

    #[tokio::test]
    async fn a_following_device_sends_no_final_report() {
        let (origin, recorded) = serve_shared_playback(WITH_SHARED_SESSION, "200 OK").await;
        let (service, local_sender) = test_service(origin);
        service.on_server_snapshot(snapshot(10, Some("another-device")));
        local_sender
            .send(Some(progress(true, 10.0, "track:t1:")))
            .expect("local state");

        service.shutdown_report().await;

        assert!(recorded.lock().expect("recorded").is_empty());
    }

    #[tokio::test]
    async fn a_silent_server_cannot_hold_the_exit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind silent server");
        let origin = ServerOrigin::parse(&format!(
            "http://{}",
            listener.local_addr().expect("address")
        ))
        .expect("origin");
        let silent = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let (service, local_sender) = test_service(origin);
        playing_as_active_device(&service, &local_sender).await;

        let started = Instant::now();
        service.shutdown_report().await;

        assert!(started.elapsed() < super::SHUTDOWN_REPORT_TIMEOUT + Duration::from_millis(500));
        silent.abort();
    }
}
