use earthly_audio_core::adaptive_system_rate::{
    AdaptiveCleanupMarker, AdaptiveSystemRateController, CommandPipeWireRateAdapter,
    FileAdaptiveCleanupMarker, PipeWireRateAdapter,
};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Prevent concurrent process spawning from inheriting another fixture's writable executable.
static TEMPORARY_EXECUTABLE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct RecordedPipeWireRate {
    forced_rate_hz: Mutex<Option<u32>>,
}

impl PipeWireRateAdapter for RecordedPipeWireRate {
    fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
        self.forced_rate_hz
            .lock()
            .map(|rate| *rate)
            .map_err(|_| "controlled PipeWire state failure".to_owned())
    }

    fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
        *self
            .forced_rate_hz
            .lock()
            .map_err(|_| "controlled PipeWire state failure".to_owned())? = rate_hz;
        Ok(())
    }
}

struct FailingResetPipeWireRate;

struct RecordedCleanupMarker {
    is_required: Mutex<bool>,
}

impl RecordedCleanupMarker {
    fn new(is_required: bool) -> Self {
        Self {
            is_required: Mutex::new(is_required),
        }
    }
}

impl AdaptiveCleanupMarker for RecordedCleanupMarker {
    fn is_required(&self) -> Result<bool, String> {
        Ok(*self.is_required.lock().expect("cleanup marker"))
    }

    fn mark_required(&self) -> Result<(), String> {
        *self.is_required.lock().expect("cleanup marker") = true;
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        *self.is_required.lock().expect("cleanup marker") = false;
        Ok(())
    }
}

impl PipeWireRateAdapter for FailingResetPipeWireRate {
    fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
        Ok(Some(96_000))
    }

    fn set_forced_rate_hz(&self, _rate_hz: Option<u32>) -> Result<(), String> {
        Err("controlled PipeWire reset failure".to_owned())
    }
}

#[test]
fn adaptive_mode_enables_without_a_confirmation_payload() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    let mut controller = AdaptiveSystemRateController::new(adapter);

    controller.enable().expect("enable Adaptive System Rate");
    assert!(controller.state().is_enabled);
}

#[test]
fn enabled_mode_marks_cleanup_required_until_a_clean_disable() {
    let marker = Arc::new(RecordedCleanupMarker::new(false));
    let mut controller = AdaptiveSystemRateController::with_cleanup_marker(
        Arc::new(RecordedPipeWireRate::default()),
        marker.clone(),
    );

    controller.enable().expect("enable Adaptive System Rate");
    assert!(marker.is_required().expect("cleanup marker is set"));

    controller.disable().expect("disable adaptive mode");
    assert!(!marker.is_required().expect("cleanup marker is clear"));
}

#[test]
fn file_cleanup_marker_survives_until_explicitly_cleared() {
    let path = temporary_path("adaptive-cleanup-marker");
    let marker = FileAdaptiveCleanupMarker::new(path.clone());

    assert!(!marker.is_required().expect("marker starts absent"));
    marker.mark_required().expect("persist cleanup marker");
    assert!(marker.is_required().expect("marker survives on disk"));
    marker.clear().expect("clear cleanup marker");
    assert!(!path.exists());
}

#[test]
fn enabled_mode_forces_the_playback_source_sample_rate() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    let mut controller = AdaptiveSystemRateController::new(adapter.clone());
    controller.enable().expect("enable Adaptive System Rate");

    controller
        .apply_source_sample_rate(Some(44_100))
        .expect("force Playback Source rate");

    assert_eq!(
        adapter.forced_rate_hz().expect("observe force-rate"),
        Some(44_100)
    );
    assert_eq!(controller.state().forced_rate_hz, Some(44_100));
}

#[test]
fn changing_playback_sources_updates_the_forced_graph_rate() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    let mut controller = AdaptiveSystemRateController::new(adapter.clone());
    controller.enable().expect("enable Adaptive System Rate");
    controller
        .apply_source_sample_rate(Some(44_100))
        .expect("force first source rate");

    controller
        .apply_source_sample_rate(Some(96_000))
        .expect("force next source rate");

    assert_eq!(
        adapter.forced_rate_hz().expect("observe force-rate"),
        Some(96_000)
    );
}

#[test]
fn disabling_adaptive_mode_restores_automatic_system_rate() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    let mut controller = AdaptiveSystemRateController::new(adapter.clone());
    controller.enable().expect("enable Adaptive System Rate");
    controller
        .apply_source_sample_rate(Some(192_000))
        .expect("force source rate");

    controller.disable().expect("disable adaptive mode");

    assert_eq!(
        adapter.forced_rate_hz().expect("observe automatic rate"),
        None
    );
    assert_eq!(
        controller.state(),
        &Default::default(),
        "disabled state reports no stale force-rate"
    );
}

#[test]
fn startup_recovery_clears_a_rate_left_by_an_abnormal_exit() {
    let adapter = Arc::new(RecordedPipeWireRate {
        forced_rate_hz: Mutex::new(Some(88_200)),
    });

    let controller =
        AdaptiveSystemRateController::recover_startup(adapter.clone()).expect("recover startup");

    assert_eq!(
        adapter.forced_rate_hz().expect("observe automatic rate"),
        None
    );
    assert_eq!(controller.state(), &Default::default());
}

#[test]
fn startup_recovery_propagates_a_stale_force_rate_reset_failure() {
    let error =
        match AdaptiveSystemRateController::recover_startup(Arc::new(FailingResetPipeWireRate)) {
            Ok(_) => panic!("stale force-rate reset must be safety-critical"),
            Err(error) => error,
        };

    assert_eq!(error, "controlled PipeWire reset failure");
}

#[test]
fn startup_without_an_adaptive_cleanup_marker_does_not_require_pipewire() {
    let missing_binary = temporary_path("missing-pipewire");
    let adapter = Arc::new(CommandPipeWireRateAdapter::with_binary(missing_binary));
    let marker = Arc::new(RecordedCleanupMarker::new(false));

    let controller = AdaptiveSystemRateController::recover_startup_if_marked(adapter, marker)
        .expect("start without optional PipeWire tools");

    assert_eq!(controller.state(), &Default::default());
}

#[test]
fn known_stale_cleanup_failure_blocks_startup_and_keeps_the_marker() {
    let marker = Arc::new(RecordedCleanupMarker::new(true));
    let error = match AdaptiveSystemRateController::recover_startup_if_marked(
        Arc::new(FailingResetPipeWireRate),
        marker.clone(),
    ) {
        Ok(_) => panic!("known stale force-rate must block startup when cleanup fails"),
        Err(error) => error,
    };

    assert_eq!(error, "controlled PipeWire reset failure");
    assert!(marker.is_required().expect("cleanup marker remains set"));
}

#[test]
fn player_recovery_resets_force_rate_without_forgetting_the_enabled_mode() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    let mut controller = AdaptiveSystemRateController::new(adapter.clone());
    controller.enable().expect("enable Adaptive System Rate");
    controller
        .apply_source_sample_rate(Some(48_000))
        .expect("force source rate");

    controller
        .reset_for_recovery()
        .expect("reset before player recovery");

    assert_eq!(
        adapter.forced_rate_hz().expect("observe automatic rate"),
        None
    );
    assert!(controller.state().is_enabled);
    assert_eq!(controller.state().forced_rate_hz, None);
}

#[test]
fn teardown_restores_automatic_rate_after_an_interrupted_session() {
    let adapter = Arc::new(RecordedPipeWireRate::default());
    {
        let mut controller = AdaptiveSystemRateController::new(adapter.clone());
        controller.enable().expect("enable Adaptive System Rate");
        controller
            .apply_source_sample_rate(Some(352_800))
            .expect("force source rate");
    }

    assert_eq!(
        adapter.forced_rate_hz().expect("observe automatic rate"),
        None
    );
}

#[test]
fn command_adapter_reads_only_pipewire_force_rate_metadata() {
    let _temporary_executable_guard = TEMPORARY_EXECUTABLE_LOCK
        .lock()
        .expect("lock temporary executable fixture");
    let log_path = temporary_path("force-rate-query-arguments");
    let script = temporary_executable(
        "read-force-rate",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s\\n' '[{{\"props\":{{\"metadata.name\":\"settings\"}},\"metadata\":[{{\"subject\":0,\"key\":\"clock.force-rate\",\"value\":176400}}]}}]'\n",
            log_path.display()
        ),
    );
    let adapter = CommandPipeWireRateAdapter::with_binaries(script.clone(), script.clone());

    assert_eq!(
        adapter.forced_rate_hz().expect("read force-rate"),
        Some(176_400)
    );
    assert_eq!(
        std::fs::read_to_string(&log_path).expect("read query arguments"),
        "-N\n"
    );

    std::fs::remove_file(script).expect("remove fake pw-metadata");
    std::fs::remove_file(log_path).expect("remove argument log");
}

#[test]
fn missing_pipewire_observer_is_not_silently_treated_as_automatic_rate() {
    let missing_dump = temporary_path("missing-pw-dump");
    let adapter = CommandPipeWireRateAdapter::with_binaries(
        PathBuf::from("pw-metadata"),
        missing_dump.clone(),
    );

    let error = adapter
        .forced_rate_hz()
        .expect_err("surface missing PipeWire observer");

    assert!(error.contains("query PipeWire clock.force-rate"));
    assert!(error.contains(&missing_dump.display().to_string()));
}

#[test]
fn command_adapter_sets_source_rate_and_resets_to_automatic() {
    let _temporary_executable_guard = TEMPORARY_EXECUTABLE_LOCK
        .lock()
        .expect("lock temporary executable fixture");
    let log_path = temporary_path("force-rate-arguments");
    let script = temporary_executable(
        "write-force-rate",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            log_path.display()
        ),
    );
    let adapter = CommandPipeWireRateAdapter::with_binary(script.clone());

    adapter
        .set_forced_rate_hz(Some(48_000))
        .expect("force source rate");
    adapter
        .set_forced_rate_hz(None)
        .expect("restore automatic rate");

    let arguments = std::fs::read_to_string(&log_path).expect("read pw-metadata arguments");
    assert_eq!(
        arguments.lines().collect::<Vec<_>>(),
        [
            "-n settings 0 clock.force-rate 48000",
            "-n settings 0 clock.force-rate 0"
        ]
    );
    std::fs::remove_file(script).expect("remove fake pw-metadata");
    std::fs::remove_file(log_path).expect("remove argument log");
}

#[test]
#[ignore = "temporarily changes the active PipeWire graph rate"]
fn real_pipewire_adapter_tracks_source_rates_and_restores_automatic() {
    let adapter = Arc::new(CommandPipeWireRateAdapter::new());
    let mut controller = AdaptiveSystemRateController::new(adapter.clone());
    controller.enable().expect("enable Adaptive System Rate");

    for rate_hz in [44_100, 96_000, 192_000] {
        controller
            .apply_source_sample_rate(Some(rate_hz))
            .expect("force playback source rate");
        assert_eq!(
            adapter.forced_rate_hz().expect("observe forced rate"),
            Some(rate_hz)
        );
    }

    controller.disable().expect("restore automatic system rate");
    assert_eq!(
        adapter.forced_rate_hz().expect("observe automatic rate"),
        None
    );
}

fn temporary_executable(name: &str, contents: &str) -> PathBuf {
    let path = temporary_path(name);
    std::fs::write(&path, contents).expect("write fake pw-metadata");
    let mut permissions = std::fs::metadata(&path)
        .expect("read fake pw-metadata metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("make fake pw-metadata executable");
    path
}

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "earthly-audio-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("read system time")
            .as_nanos()
    ))
}
