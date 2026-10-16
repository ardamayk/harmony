use earthly_audio_core::processing::{
    EffectiveReplayGainMode, EqualizerPreset, FileProcessingSettingsStorage,
    MpvProcessingConfiguration, OutputMode, ProcessingController, ProcessingProfile,
    ProcessingSettingsStorage, ReplayGainMode,
};

struct EmptySettingsStorage;

struct FailingSettingsStorage;

impl ProcessingSettingsStorage for EmptySettingsStorage {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn save(&self, _value: &str) -> Result<(), String> {
        Ok(())
    }
}

impl ProcessingSettingsStorage for FailingSettingsStorage {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn save(&self, _value: &str) -> Result<(), String> {
        Err("controlled settings failure".to_owned())
    }
}

fn in_memory_controller() -> ProcessingController {
    ProcessingController::open(Box::new(EmptySettingsStorage))
        .expect("open in-memory Processing Profile")
}

#[test]
fn direct_profile_exposes_unity_processing_to_mpv() {
    let controller = in_memory_controller();

    assert_eq!(controller.state().profile, ProcessingProfile::Direct);
    assert_eq!(
        controller.state().effective_replay_gain_mode,
        EffectiveReplayGainMode::Off
    );
    assert_eq!(
        controller.mpv_configuration(),
        MpvProcessingConfiguration {
            volume_percent: 100.0,
            replay_gain_mode: ReplayGainMode::Off,
            audio_filters: Vec::new(),
        }
    );
}

#[test]
fn failed_persistence_does_not_mutate_observable_processing_state() {
    let mut controller = ProcessingController::open(Box::new(FailingSettingsStorage))
        .expect("open failing storage fixture");

    let error = controller
        .set_software_volume(0.4)
        .expect_err("surface persistence failure");

    assert!(error.contains("controlled settings failure"));
    assert_eq!(controller.state().profile, ProcessingProfile::Direct);
    assert_eq!(controller.state().software_volume, 1.0);
}

#[test]
fn user_processing_changes_switch_profile_and_persist() {
    let settings_path = temporary_path("processing-settings.json");
    let storage = FileProcessingSettingsStorage::new(settings_path.clone());
    let mut controller =
        ProcessingController::open(Box::new(storage)).expect("open Processing Profile");

    controller
        .set_software_volume(0.45)
        .expect("set software volume");
    assert_eq!(
        controller.state().transition_notice.as_deref(),
        Some("Software volume requires the Processed Profile.")
    );
    controller
        .enable_replay_gain(ReplayGainMode::Album)
        .expect("enable ReplayGain");
    controller
        .apply_equalizer_preset(EqualizerPreset::Vocal)
        .expect("apply EQ preset");

    assert_eq!(controller.state().profile, ProcessingProfile::Processed);
    assert_eq!(controller.mpv_configuration().audio_filters.len(), 10);

    let mut restored = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("restore Processing Profile");
    assert_eq!(restored.state().software_volume, 0.45);
    assert_eq!(restored.state().replay_gain_mode, ReplayGainMode::Album);
    assert_eq!(restored.state().equalizer.preset, EqualizerPreset::Vocal);
    restored
        .set_equalizer_gain(0, 6.0)
        .expect("set custom EQ gain");
    assert_eq!(restored.state().equalizer.preset, EqualizerPreset::Custom);
    restored
        .set_profile(ProcessingProfile::Direct)
        .expect("restore Direct Profile");
    assert_eq!(
        restored.mpv_configuration(),
        MpvProcessingConfiguration {
            volume_percent: 100.0,
            replay_gain_mode: ReplayGainMode::Off,
            audio_filters: Vec::new(),
        }
    );

    std::fs::remove_file(settings_path).expect("remove settings fixture");
}

#[test]
fn processing_mutations_preserve_the_persisted_system_output_mode() {
    let settings_path = temporary_path("output-mode-settings.json");
    let mut controller = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("open audio settings");

    assert_eq!(controller.output_mode(), OutputMode::System);
    controller
        .set_software_volume(0.55)
        .expect("change Processing Profile state");
    controller
        .enable_replay_gain(ReplayGainMode::Track)
        .expect("change ReplayGain state");

    assert_eq!(controller.output_mode(), OutputMode::System);
    let persisted = std::fs::read_to_string(&settings_path).expect("read audio settings");
    assert!(persisted.contains(r#""outputMode":"system""#));
    let restored = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("restore audio settings");
    assert_eq!(restored.output_mode(), OutputMode::System);

    std::fs::remove_file(settings_path).expect("remove settings fixture");
}

#[test]
fn direct_alsa_selection_persists_independently_from_processing() {
    let settings_path = temporary_path("direct-alsa-output-settings.json");
    let mut controller = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("open audio settings");

    controller
        .select_direct_alsa_output("hw:2,0")
        .expect("persist Direct ALSA Output and selected DAC");
    controller
        .set_software_volume(0.55)
        .expect("change Processing Profile state");

    let restored = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("restore audio settings");
    assert_eq!(restored.output_mode(), OutputMode::DirectAlsa);
    assert_eq!(restored.selected_output_device_id(), Some("hw:2,0"));
    assert_eq!(restored.state().profile, ProcessingProfile::Processed);

    std::fs::remove_file(settings_path).expect("remove settings fixture");
}

#[test]
fn normal_output_replaces_and_persists_previous_native_modes() {
    let settings_path = temporary_path("normal-output-settings.json");
    let mut controller = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("open audio settings");

    controller
        .select_direct_alsa_output("hw:2,0")
        .expect("persist Exclusive Output");
    controller
        .set_output_mode(OutputMode::System)
        .expect("persist Normal Output");
    assert_eq!(controller.output_mode(), OutputMode::System);

    let mut restored = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("restore Normal Output");
    assert_eq!(restored.output_mode(), OutputMode::System);
    restored
        .set_output_mode(OutputMode::AdaptiveSystemRate)
        .expect("persist Adaptive System Rate");
    restored
        .set_output_mode(OutputMode::System)
        .expect("restore Normal Output again");

    let final_state = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(
        settings_path.clone(),
    )))
    .expect("restore final Normal Output");
    assert_eq!(final_state.output_mode(), OutputMode::System);

    std::fs::remove_file(settings_path).expect("remove settings fixture");
}

#[test]
fn failed_direct_alsa_persistence_keeps_the_previous_output_selection() {
    let mut controller = ProcessingController::open(Box::new(FailingSettingsStorage))
        .expect("open failing storage fixture");

    controller
        .select_direct_alsa_output("hw:2,0")
        .expect_err("surface persistence failure");

    assert_eq!(controller.output_mode(), OutputMode::System);
    assert_eq!(controller.selected_output_device_id(), None);
}

fn temporary_path(file_name: &str) -> std::path::PathBuf {
    let unique = format!(
        "earthly-audio-{}-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test"),
        file_name
    );
    std::env::temp_dir().join(unique)
}

#[test]
fn software_volume_is_remembered_per_output_route() {
    let path = temporary_path("volume-memory.json");
    let storage = FileProcessingSettingsStorage::new(path.clone());
    let mut controller =
        ProcessingController::open(Box::new(storage)).expect("open file-backed controller");

    controller
        .set_software_volume(0.4)
        .expect("set system volume");
    controller
        .select_direct_alsa_output("hw:1,0")
        .expect("select direct output");
    assert_eq!(controller.remembered_volume(), None);
    controller
        .set_software_volume(0.9)
        .expect("set direct volume");
    assert_eq!(controller.remembered_volume(), Some(0.9));

    controller
        .set_output_mode(OutputMode::System)
        .expect("back to system");
    assert_eq!(controller.remembered_volume(), Some(0.4));

    let reopened = ProcessingController::open(Box::new(FileProcessingSettingsStorage::new(path)))
        .expect("reopen controller");
    assert_eq!(reopened.remembered_volume(), Some(0.4));
    assert_eq!(
        earthly_audio_core::processing::output_volume_key(OutputMode::DirectAlsa, Some("hw:1,0")),
        "alsa:hw:1,0"
    );
}

#[test]
fn failed_volume_persistence_does_not_remember_the_volume() {
    let mut controller = ProcessingController::open(Box::new(FailingSettingsStorage))
        .expect("open failing storage fixture");
    assert!(controller.set_software_volume(0.3).is_err());
    assert_eq!(controller.remembered_volume(), None);
}
