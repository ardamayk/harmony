//! Windows audio: the WASAPI (Core Audio) counterparts of the Linux PipeWire
//! and ALSA adapters.
//!
//! - Normal output is mpv's `wasapi` output on the default endpoint.
//! - Exclusive output resolves the default render endpoint and hands its id
//!   to mpv as `wasapi/<endpoint id>` with `audio-exclusive`; WASAPI exclusive
//!   mode needs no card-profile juggling, so its coordinator holds no lease.
//! - Device volume is the endpoint's own volume (`IAudioEndpointVolume`).
//! - Adaptive System Rate changes the endpoint's shared-mode format
//!   (`policy_config`); there is no graph to observe for PipeWire telemetry.

use crate::device_volume::{DeviceVolume, VolumeRoute};
use crate::exclusive_output::ExclusiveOutputCoordinator;
use crate::output_device::{
    ActiveOutputError, ActiveOutputResolver, AlsaOutputDeviceAdapter, OutputDevice,
};
use crate::telemetry::{PipeWireObservation, PipeWireObserver};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{ERROR_NOT_FOUND, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, ENDPOINT_HARDWARE_SUPPORT_VOLUME, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};

/// Initializes COM on the calling thread for one call. A thread that already
/// joined an apartment keeps it; COM is only uninitialized when this call
/// initialized it.
pub(crate) struct ComScope {
    owns_initialization: bool,
}

impl ComScope {
    pub(crate) fn enter() -> Result<Self, String> {
        // SAFETY: paired with CoUninitialize in Drop when it succeeded.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return Ok(Self {
                owns_initialization: true,
            });
        }
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                owns_initialization: false,
            });
        }
        Err(format!("Windows audio could not start COM: {result}"))
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.owns_initialization {
            // SAFETY: this scope's CoInitializeEx succeeded on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    // SAFETY: COM is initialized by the caller's ComScope.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

pub(crate) fn default_render_device(
    enumerator: &IMMDeviceEnumerator,
) -> windows::core::Result<IMMDevice> {
    // SAFETY: COM is initialized by the caller's ComScope.
    unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
}

/// The endpoint id mpv and IPolicyConfig take, such as `{0.0.0.00000000}.{guid}`.
pub(crate) fn endpoint_id(device: &IMMDevice) -> windows::core::Result<String> {
    // SAFETY: GetId returns a CoTaskMem string that is freed after copying.
    unsafe {
        let raw = device.GetId()?;
        let id = raw.to_string();
        CoTaskMemFree(Some(raw.0.cast()));
        Ok(id?)
    }
}

fn endpoint(device: &IMMDevice) -> windows::core::Result<OutputDevice> {
    let id = endpoint_id(device)?;
    let name = friendly_name(device).unwrap_or_else(|| id.clone());
    Ok(OutputDevice { id, name })
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: COM is initialized by the caller's ComScope.
    let value = unsafe {
        device
            .OpenPropertyStore(STGM_READ)
            .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
            .ok()?
    };
    let name = value.to_string();
    (!name.is_empty()).then_some(name)
}

/// Resolves the default render endpoint, the device Exclusive opens.
pub struct WasapiActiveOutputResolver;

impl WasapiActiveOutputResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WasapiActiveOutputResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveOutputResolver for WasapiActiveOutputResolver {
    fn resolve(&self) -> Result<OutputDevice, ActiveOutputError> {
        let _com = ComScope::enter().map_err(ActiveOutputError::QueryFailed)?;
        let enumerator =
            enumerator().map_err(|error| ActiveOutputError::QueryFailed(error.to_string()))?;
        let device = match default_render_device(&enumerator) {
            Ok(device) => device,
            // No render endpoint is enabled.
            Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => {
                return Err(ActiveOutputError::NoActiveOutput);
            }
            Err(error) => return Err(ActiveOutputError::QueryFailed(error.to_string())),
        };
        endpoint(&device).map_err(|error| ActiveOutputError::QueryFailed(error.to_string()))
    }
}

/// Lists the active render endpoints that Exclusive can open.
pub struct WasapiOutputDeviceAdapter;

impl WasapiOutputDeviceAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WasapiOutputDeviceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AlsaOutputDeviceAdapter for WasapiOutputDeviceAdapter {
    fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String> {
        let _com = ComScope::enter()?;
        let list = || -> windows::core::Result<Vec<OutputDevice>> {
            let enumerator = enumerator()?;
            // SAFETY: COM is initialized by the ComScope above.
            unsafe {
                let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
                let count = collection.GetCount()?;
                let mut devices = Vec::with_capacity(count as usize);
                for index in 0..count {
                    devices.push(endpoint(&collection.Item(index)?)?);
                }
                Ok(devices)
            }
        };
        list().map_err(|error| format!("Failed to enumerate Windows audio endpoints: {error}"))
    }
}

/// WASAPI exclusive mode is opened by mpv itself (`audio-exclusive`), and the
/// endpoint is released when mpv closes its stream, so there is nothing to
/// acquire or restore around it.
pub(crate) struct WasapiExclusiveOutputCoordinator;

impl WasapiExclusiveOutputCoordinator {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for WasapiExclusiveOutputCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ExclusiveOutputCoordinator for WasapiExclusiveOutputCoordinator {
    fn acquire(&self, _device_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn release(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Moves the default render endpoint's volume, only where the endpoint applies
/// it in hardware. There the stream reaching it keeps its samples, which is
/// what the hand-off is for; a software endpoint volume would change them like
/// mpv's does, so such an endpoint takes nothing and mpv keeps the volume.
/// Exclusive always opens the default endpoint, so every route lands on it.
///
/// The endpoint volume is system-wide, so `lib.rs` records each hand-off on
/// disk and undoes one a killed client left behind at the next start.
pub(crate) struct WasapiDeviceVolume;

impl DeviceVolume for WasapiDeviceVolume {
    fn change_db(&self, _route: VolumeRoute, delta_db: f64) -> Result<f64, String> {
        let _com = ComScope::enter()?;
        let device = default_render_device(&enumerator().map_err(|error| error.to_string())?)
            .map_err(|error| format!("The default output could not be found: {error}"))?;
        change_endpoint_db(&device, delta_db, true)
    }
}

/// The default render endpoint's id, which a hand-off record names.
pub(crate) fn default_endpoint_id() -> Result<String, String> {
    let _com = ComScope::enter()?;
    let device = default_render_device(&enumerator().map_err(|error| error.to_string())?)
        .map_err(|error| format!("The default output could not be found: {error}"))?;
    endpoint_id(&device).map_err(|error| error.to_string())
}

/// Changes a named endpoint's volume by `delta_db`, hardware or not: used to
/// undo a recorded hand-off. `Ok(false)` while the endpoint is absent.
pub(crate) fn change_named_endpoint_db(device_id: &str, delta_db: f64) -> Result<bool, String> {
    let _com = ComScope::enter()?;
    // SAFETY: COM is initialized by the ComScope above.
    let device = unsafe {
        enumerator()
            .and_then(|enumerator| enumerator.GetDevice(&windows_core::HSTRING::from(device_id)))
    };
    let Ok(device) = device else {
        return Ok(false);
    };
    // SAFETY: COM is initialized by the ComScope above.
    if unsafe { device.GetState() }.ok() != Some(DEVICE_STATE_ACTIVE) {
        return Ok(false);
    }
    change_endpoint_db(&device, delta_db, false).map(|_| true)
}

/// Returns the change the endpoint made, which is smaller at its limits, and
/// none when `hardware_only` and the endpoint has no hardware volume.
fn change_endpoint_db(
    device: &IMMDevice,
    delta_db: f64,
    hardware_only: bool,
) -> Result<f64, String> {
    let change = || -> windows::core::Result<f64> {
        // SAFETY: COM is initialized by the caller's ComScope; the volume
        // interface is used only while `device` is alive.
        unsafe {
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            if hardware_only
                && volume.QueryHardwareSupport()? & ENDPOINT_HARDWARE_SUPPORT_VOLUME == 0
            {
                return Ok(0.0);
            }
            let (mut minimum, mut maximum, mut _step) = (0.0_f32, 0.0_f32, 0.0_f32);
            volume.GetVolumeRange(&mut minimum, &mut maximum, &mut _step)?;
            let before = volume.GetMasterVolumeLevel()?;
            let target =
                (f64::from(before) + delta_db).clamp(f64::from(minimum), f64::from(maximum)) as f32;
            volume.SetMasterVolumeLevel(target, std::ptr::null())?;
            Ok(f64::from(volume.GetMasterVolumeLevel()?) - f64::from(before))
        }
    };
    change().map_err(|error| format!("The output device volume could not be changed: {error}"))
}

/// There is no PipeWire graph on Windows; the path stays unobserved.
pub struct NoSystemPathObserver;

impl NoSystemPathObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoSystemPathObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeWireObserver for NoSystemPathObserver {
    fn observe(&self) -> Result<Option<PipeWireObservation>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{ComScope, default_render_device, enumerator};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::CLSCTX_ALL;

    #[test]
    #[ignore = "reads the real default output's volume and mute"]
    fn real_default_output_volume_and_mute() {
        let _com = ComScope::enter().expect("COM");
        let device = default_render_device(&enumerator().unwrap()).expect("default output");
        // SAFETY: COM is initialized above; `device` outlives the interface.
        unsafe {
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).unwrap();
            eprintln!(
                "default output: {:.1} dB, scalar {:.3}, mute {}, hardware support {:#x}",
                volume.GetMasterVolumeLevel().unwrap(),
                volume.GetMasterVolumeLevelScalar().unwrap(),
                volume.GetMute().unwrap().as_bool(),
                volume.QueryHardwareSupport().unwrap()
            );
        }
    }
}
