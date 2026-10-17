use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub const EQ_FREQUENCIES_HZ: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessingProfile {
    Direct,
    Processed,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[default]
    System,
    DirectAlsa,
    AdaptiveSystemRate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    Off,
    Track,
    Album,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectiveReplayGainMode {
    Off,
    Track,
    Album,
    TrackFallback,
    Unavailable,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EqualizerPreset {
    Flat,
    BassBoost,
    Vocal,
    TrebleBoost,
    Custom,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqualizerState {
    pub is_enabled: bool,
    pub preset: EqualizerPreset,
    pub gains_db: [f64; 10],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingState {
    pub profile: ProcessingProfile,
    pub software_volume: f64,
    pub replay_gain_mode: ReplayGainMode,
    pub effective_replay_gain_mode: EffectiveReplayGainMode,
    pub replay_gain_preference: Option<ReplayGainMode>,
    pub equalizer: EqualizerState,
    pub effective_audio_filters: Vec<String>,
    pub transition_notice: Option<String>,
}

impl Default for ProcessingState {
    fn default() -> Self {
        default_state()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedProcessingSettings {
    #[serde(default)]
    output_mode: OutputMode,
    #[serde(default)]
    selected_output_device_id: Option<String>,
    /// Last software volume per output route, keyed by `output_volume_key`.
    #[serde(default)]
    volume_by_output_device: BTreeMap<String, f64>,
    profile: ProcessingProfile,
    software_volume: f64,
    replay_gain_mode: ReplayGainMode,
    replay_gain_preference: Option<ReplayGainMode>,
    equalizer: EqualizerState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MpvProcessingConfiguration {
    pub volume_percent: f64,
    pub replay_gain_mode: ReplayGainMode,
    pub audio_filters: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppliedMpvProcessingConfiguration {
    pub volume_percent: f64,
    pub replay_gain_mode: ReplayGainMode,
    pub audio_filters: Vec<String>,
}

pub trait ProcessingSettingsStorage: Send + Sync {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, value: &str) -> Result<(), String>;
}

pub struct FileProcessingSettingsStorage {
    path: PathBuf,
}

impl FileProcessingSettingsStorage {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ProcessingSettingsStorage for FileProcessingSettingsStorage {
    fn load(&self) -> Result<Option<String>, String> {
        match fs::read_to_string(&self.path) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "Failed to read Processing Profile settings from {}: {error}",
                self.path.display()
            )),
        }
    }

    fn save(&self, value: &str) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create settings directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let temporary_path = self.path.with_extension("json.tmp");
        fs::write(&temporary_path, value).map_err(|error| {
            format!(
                "Failed to persist Processing Profile settings to {}: {error}",
                temporary_path.display()
            )
        })?;
        fs::rename(&temporary_path, &self.path).map_err(|error| {
            format!(
                "Failed to finalize Processing Profile settings at {}: {error}",
                self.path.display()
            )
        })
    }
}

pub struct ProcessingController {
    storage: Box<dyn ProcessingSettingsStorage>,
    output_mode: OutputMode,
    selected_output_device_id: Option<String>,
    volume_by_output_device: BTreeMap<String, f64>,
    state: ProcessingState,
}

pub(crate) struct ProcessingSnapshot {
    pub(crate) state: ProcessingState,
    volume_by_output_device: BTreeMap<String, f64>,
}

/// Key under which the software volume is remembered for an output route.
/// ALSA device ids are machine-local, so this memory stays in the desktop's
/// own settings file rather than in the server-side preferences.
pub fn output_volume_key(output_mode: OutputMode, device_id: Option<&str>) -> String {
    match (output_mode, device_id) {
        (OutputMode::DirectAlsa, Some(device_id)) => format!("alsa:{device_id}"),
        _ => "system".to_owned(),
    }
}

impl ProcessingController {
    pub fn open(storage: Box<dyn ProcessingSettingsStorage>) -> Result<Self, String> {
        let loaded = match storage.load()? {
            Some(value) => state_from_json(&value)?,
            None => LoadedProcessingSettings::default(),
        };
        Ok(Self {
            storage,
            output_mode: loaded.output_mode,
            selected_output_device_id: loaded.selected_output_device_id,
            volume_by_output_device: loaded.volume_by_output_device,
            state: loaded.state,
        })
    }

    /// Software volume last used on the active output route, if any.
    pub fn remembered_volume(&self) -> Option<f64> {
        self.volume_by_output_device
            .get(&output_volume_key(
                self.output_mode,
                self.selected_output_device_id.as_deref(),
            ))
            .copied()
    }

    pub fn state(&self) -> &ProcessingState {
        &self.state
    }

    pub(crate) fn snapshot(&self) -> ProcessingSnapshot {
        ProcessingSnapshot {
            state: self.state.clone(),
            volume_by_output_device: self.volume_by_output_device.clone(),
        }
    }

    pub(crate) fn restore_snapshot(&mut self, snapshot: ProcessingSnapshot) -> Result<(), String> {
        self.state = snapshot.state;
        self.volume_by_output_device = snapshot.volume_by_output_device;
        self.persist_state(&self.state)
    }

    pub fn output_mode(&self) -> OutputMode {
        self.output_mode
    }

    pub fn selected_output_device_id(&self) -> Option<&str> {
        self.selected_output_device_id.as_deref()
    }

    pub fn set_output_mode(&mut self, output_mode: OutputMode) -> Result<(), String> {
        self.persist_settings(
            &self.state,
            output_mode,
            self.selected_output_device_id.as_deref(),
        )?;
        self.output_mode = output_mode;
        Ok(())
    }

    pub fn select_output_device(&mut self, device_id: &str) -> Result<(), String> {
        self.persist_settings(&self.state, self.output_mode, Some(device_id))?;
        self.selected_output_device_id = Some(device_id.to_owned());
        Ok(())
    }

    pub fn select_direct_alsa_output(&mut self, device_id: &str) -> Result<(), String> {
        self.persist_settings(&self.state, OutputMode::DirectAlsa, Some(device_id))?;
        self.output_mode = OutputMode::DirectAlsa;
        self.selected_output_device_id = Some(device_id.to_owned());
        Ok(())
    }

    pub fn set_profile(&mut self, profile: ProcessingProfile) -> Result<(), String> {
        self.commit(|state| {
            state.profile = profile;
            state.transition_notice = None;
            if profile == ProcessingProfile::Direct {
                clear_active_processing(state);
            }
            Ok(())
        })
    }

    pub fn set_software_volume(&mut self, value: f64) -> Result<(), String> {
        let volume = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let key = output_volume_key(self.output_mode, self.selected_output_device_id.as_deref());
        let previous = self.volume_by_output_device.insert(key.clone(), volume);
        let result = self.commit(|state| {
            let is_transition = state.profile == ProcessingProfile::Direct && volume != 1.0;
            state.software_volume = volume;
            transition_to_processed(
                state,
                is_transition,
                "Software volume requires the Processed Profile.",
            );
            Ok(())
        });
        if result.is_err() {
            match previous {
                Some(previous) => self.volume_by_output_device.insert(key, previous),
                None => self.volume_by_output_device.remove(&key),
            };
        }
        result
    }

    pub fn enable_replay_gain(&mut self, mode: ReplayGainMode) -> Result<(), String> {
        if mode == ReplayGainMode::Off {
            return self.disable_replay_gain();
        }
        self.commit(|state| {
            let is_transition = state.profile == ProcessingProfile::Direct;
            state.replay_gain_mode = mode;
            state.effective_replay_gain_mode = EffectiveReplayGainMode::Unknown;
            state.replay_gain_preference = Some(mode);
            transition_to_processed(
                state,
                is_transition,
                "ReplayGain requires the Processed Profile.",
            );
            Ok(())
        })
    }

    pub fn disable_replay_gain(&mut self) -> Result<(), String> {
        self.commit(|state| {
            state.replay_gain_mode = ReplayGainMode::Off;
            state.effective_replay_gain_mode = EffectiveReplayGainMode::Off;
            state.transition_notice = None;
            Ok(())
        })
    }

    pub fn apply_equalizer_preset(&mut self, preset: EqualizerPreset) -> Result<(), String> {
        let gains_db = preset_gains(preset);
        let is_enabled = preset != EqualizerPreset::Flat;
        self.commit(|state| {
            let is_transition = state.profile == ProcessingProfile::Direct && is_enabled;
            state.equalizer = EqualizerState {
                is_enabled,
                preset,
                gains_db,
            };
            transition_to_processed(
                state,
                is_transition,
                "Equalizer changes require the Processed Profile.",
            );
            Ok(())
        })
    }

    pub fn set_equalizer_gain(&mut self, index: usize, value: f64) -> Result<(), String> {
        self.commit(|state| {
            let Some(gain) = state.equalizer.gains_db.get_mut(index) else {
                return Err(format!("Equalizer band index {index} is out of range."));
            };
            *gain = if value.is_finite() {
                value.clamp(-12.0, 12.0)
            } else {
                0.0
            };
            state.equalizer.is_enabled = true;
            state.equalizer.preset = EqualizerPreset::Custom;
            transition_to_processed(
                state,
                state.profile == ProcessingProfile::Direct,
                "Equalizer changes require the Processed Profile.",
            );
            Ok(())
        })
    }

    pub fn mpv_configuration(&self) -> MpvProcessingConfiguration {
        mpv_configuration_for(&self.state)
    }

    pub fn restore(&mut self, state: ProcessingState) -> Result<(), String> {
        self.persist_state(&state)?;
        self.state = state;
        Ok(())
    }

    fn commit(
        &mut self,
        change: impl FnOnce(&mut ProcessingState) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut candidate = self.state.clone();
        change(&mut candidate)?;
        self.persist_state(&candidate)?;
        self.state = candidate;
        Ok(())
    }

    fn persist_state(&self, state: &ProcessingState) -> Result<(), String> {
        self.persist_settings(
            state,
            self.output_mode,
            self.selected_output_device_id.as_deref(),
        )
    }

    fn persist_settings(
        &self,
        state: &ProcessingState,
        output_mode: OutputMode,
        selected_output_device_id: Option<&str>,
    ) -> Result<(), String> {
        let settings = PersistedProcessingSettings::from_state(
            state,
            output_mode,
            selected_output_device_id,
            &self.volume_by_output_device,
        );
        let value = serde_json::to_string(&settings)
            .map_err(|error| format!("Failed to serialize Processing Profile settings: {error}"))?;
        self.storage.save(&value)
    }
}

pub fn mpv_configuration_for(state: &ProcessingState) -> MpvProcessingConfiguration {
    let audio_filters = if state.equalizer.is_enabled {
        create_equalizer_filters(&state.equalizer.gains_db)
    } else {
        Vec::new()
    };
    MpvProcessingConfiguration {
        volume_percent: state.software_volume * 100.0,
        replay_gain_mode: state.replay_gain_mode,
        audio_filters,
    }
}

fn transition_to_processed(state: &mut ProcessingState, is_transition: bool, notice: &str) {
    if is_transition {
        state.profile = ProcessingProfile::Processed;
        state.transition_notice = Some(notice.to_owned());
    } else {
        state.transition_notice = None;
    }
}

fn clear_active_processing(state: &mut ProcessingState) {
    state.software_volume = 1.0;
    state.replay_gain_mode = ReplayGainMode::Off;
    state.effective_replay_gain_mode = EffectiveReplayGainMode::Off;
    state.equalizer = flat_equalizer();
}

impl PersistedProcessingSettings {
    fn from_state(
        state: &ProcessingState,
        output_mode: OutputMode,
        selected_output_device_id: Option<&str>,
        volume_by_output_device: &BTreeMap<String, f64>,
    ) -> Self {
        Self {
            output_mode,
            selected_output_device_id: selected_output_device_id.map(str::to_owned),
            volume_by_output_device: volume_by_output_device.clone(),
            profile: state.profile,
            software_volume: state.software_volume,
            replay_gain_mode: state.replay_gain_mode,
            replay_gain_preference: state.replay_gain_preference,
            equalizer: state.equalizer.clone(),
        }
    }
}

struct LoadedProcessingSettings {
    state: ProcessingState,
    output_mode: OutputMode,
    selected_output_device_id: Option<String>,
    volume_by_output_device: BTreeMap<String, f64>,
}

impl Default for LoadedProcessingSettings {
    fn default() -> Self {
        Self {
            state: default_state(),
            output_mode: OutputMode::System,
            selected_output_device_id: None,
            volume_by_output_device: BTreeMap::new(),
        }
    }
}

fn state_from_json(value: &str) -> Result<LoadedProcessingSettings, String> {
    let settings: PersistedProcessingSettings = serde_json::from_str(value)
        .map_err(|error| format!("Failed to parse Processing Profile settings: {error}"))?;
    let output_mode = settings.output_mode;
    let selected_output_device_id = settings.selected_output_device_id.clone();
    let volume_by_output_device = settings.volume_by_output_device.clone();
    Ok(LoadedProcessingSettings {
        state: ProcessingState {
            profile: settings.profile,
            software_volume: settings.software_volume,
            replay_gain_mode: settings.replay_gain_mode,
            effective_replay_gain_mode: if settings.replay_gain_mode == ReplayGainMode::Off {
                EffectiveReplayGainMode::Off
            } else {
                EffectiveReplayGainMode::Unknown
            },
            replay_gain_preference: settings.replay_gain_preference,
            equalizer: settings.equalizer,
            effective_audio_filters: Vec::new(),
            transition_notice: None,
        },
        output_mode,
        selected_output_device_id,
        volume_by_output_device,
    })
}

fn default_state() -> ProcessingState {
    ProcessingState {
        profile: ProcessingProfile::Direct,
        software_volume: 1.0,
        replay_gain_mode: ReplayGainMode::Off,
        effective_replay_gain_mode: EffectiveReplayGainMode::Off,
        replay_gain_preference: None,
        equalizer: flat_equalizer(),
        effective_audio_filters: Vec::new(),
        transition_notice: None,
    }
}

fn flat_equalizer() -> EqualizerState {
    EqualizerState {
        is_enabled: false,
        preset: EqualizerPreset::Flat,
        gains_db: [0.0; 10],
    }
}

fn preset_gains(preset: EqualizerPreset) -> [f64; 10] {
    match preset {
        EqualizerPreset::Flat | EqualizerPreset::Custom => [0.0; 10],
        EqualizerPreset::BassBoost => [5.0, 4.0, 3.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        EqualizerPreset::Vocal => [-2.0, -1.0, 0.0, 1.5, 3.0, 3.0, 2.0, 1.0, 0.0, -1.0],
        EqualizerPreset::TrebleBoost => [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.5, 3.0, 4.0, 5.0],
    }
}

fn create_equalizer_filters(gains_db: &[f64; 10]) -> Vec<String> {
    EQ_FREQUENCIES_HZ
        .iter()
        .zip(gains_db)
        .map(|(frequency, gain)| format!("equalizer=f={frequency}:t=q:w=1:g={gain}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStorage(Arc<Mutex<Option<String>>>);

    impl ProcessingSettingsStorage for MemoryStorage {
        fn load(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().expect("lock settings").clone())
        }

        fn save(&self, value: &str) -> Result<(), String> {
            *self.0.lock().expect("lock settings") = Some(value.to_owned());
            Ok(())
        }
    }

    #[test]
    fn rollback_restores_existing_and_absent_route_volumes_in_memory_and_storage() {
        for remembered_volume in [None, Some(0.6)] {
            let storage = MemoryStorage::default();
            let mut controller = ProcessingController::open(Box::new(storage.clone())).unwrap();
            controller.select_direct_alsa_output("headphones").unwrap();
            controller.set_software_volume(0.3).unwrap();
            controller.set_output_mode(OutputMode::System).unwrap();
            if let Some(volume) = remembered_volume {
                controller.set_software_volume(volume).unwrap();
            }
            let snapshot = controller.snapshot();
            let previous_state = controller.state().clone();
            let previous_volumes = controller.volume_by_output_device.clone();

            controller.set_software_volume(0.9).unwrap();
            controller.restore_snapshot(snapshot).unwrap();

            assert_eq!(controller.state(), &previous_state);
            assert_eq!(controller.remembered_volume(), remembered_volume);
            assert_eq!(controller.volume_by_output_device, previous_volumes);
            let reopened = ProcessingController::open(Box::new(storage)).unwrap();
            assert_eq!(
                reopened.state().software_volume,
                previous_state.software_volume
            );
            assert_eq!(reopened.state().profile, previous_state.profile);
            assert_eq!(reopened.volume_by_output_device, previous_volumes);
        }
    }

    #[test]
    fn adaptive_output_restores_system_route_volume_after_direct_alsa() {
        let mut controller =
            ProcessingController::open(Box::new(MemoryStorage::default())).unwrap();
        controller.set_software_volume(0.4).unwrap();
        controller.select_direct_alsa_output("headphones").unwrap();
        controller.set_software_volume(0.8).unwrap();

        controller
            .set_output_mode(OutputMode::AdaptiveSystemRate)
            .unwrap();
        let volume = controller
            .remembered_volume()
            .expect("remember system volume");
        controller.set_software_volume(volume).unwrap();

        assert_eq!(controller.state().software_volume, 0.4);
        controller.select_direct_alsa_output("headphones").unwrap();
        assert_eq!(controller.remembered_volume(), Some(0.8));
    }
}
