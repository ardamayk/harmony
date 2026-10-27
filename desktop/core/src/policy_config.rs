//! Adaptive System Rate on Windows: the shared-mode format of the default
//! render endpoint follows the playing Track's sample rate.
//!
//! Windows has no documented API for this. Its Sound control panel changes an
//! endpoint's "Default Format" through `IPolicyConfig`, an undocumented COM
//! interface of the audio policy service; this module calls the same
//! interface. Only the first four methods are declared: a COM vtable is
//! position-based, so declaring nothing after `SetDeviceFormat` keeps the
//! later methods, whose shape has varied between Windows versions, out of
//! play. If a Windows release drops the interface, `ensure_available` fails
//! and Adaptive reports it; Normal and Exclusive are unaffected.
//!
//! The endpoint's original formats are saved to disk before the first change
//! and restored on reset, so a crash is undone at the next start (the cleanup
//! marker makes the controller reset).

// The interface keeps its Windows method names, which the macro also uses
// for its vtable fields.
#![allow(non_snake_case)]

use crate::adaptive_system_rate::PipeWireRateAdapter;
use crate::wasapi::{ComScope, default_render_device, endpoint_id, enumerator};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE_ACTIVE, IAudioClient, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows_core::{GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

/// `CPolicyConfigClient`, the coclass the Sound control panel uses.
const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

const UNAVAILABLE_MESSAGE: &str = "Adaptive is not available on this Windows version: its audio policy interface could not be opened. Use Exclusive for bit-perfect playback.";

/// The Windows 7+ `IPolicyConfig`, declared up to the last method used.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn GetDeviceFormat(
        &self,
        device: PCWSTR,
        default: i32,
        format: *mut *mut WAVEFORMATEX,
    ) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(
        &self,
        device: PCWSTR,
        endpoint_format: *const WAVEFORMATEX,
        mix_format: *const WAVEFORMATEX,
    ) -> HRESULT;
}

/// A `WAVEFORMATEX` and the `cbSize` extension bytes after it (for example
/// `WAVEFORMATEXTENSIBLE`), owned as plain bytes. The struct is packed, so
/// the bytes need no alignment.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WaveFormat(Vec<u8>);

impl WaveFormat {
    /// Copies a CoTaskMem-allocated format and frees the original.
    ///
    /// # Safety
    /// `format` is null or a valid format allocated with CoTaskMemAlloc.
    unsafe fn take(format: *mut WAVEFORMATEX) -> Option<Self> {
        if format.is_null() {
            return None;
        }
        // SAFETY: the caller guarantees a valid format; `cbSize` extra bytes
        // follow the header by definition.
        unsafe {
            let header = format.read_unaligned();
            let length = size_of::<WAVEFORMATEX>() + usize::from(header.cbSize);
            let bytes = std::slice::from_raw_parts(format.cast::<u8>(), length).to_vec();
            CoTaskMemFree(Some(format.cast()));
            Some(Self(bytes))
        }
    }

    fn header(&self) -> WAVEFORMATEX {
        // SAFETY: every WaveFormat holds at least a whole header.
        unsafe { self.0.as_ptr().cast::<WAVEFORMATEX>().read_unaligned() }
    }

    fn as_ptr(&self) -> *const WAVEFORMATEX {
        self.0.as_ptr().cast()
    }

    fn sample_rate_hz(&self) -> u32 {
        self.header().nSamplesPerSec
    }

    /// The same format at another rate; the byte rate follows.
    fn with_sample_rate(&self, rate_hz: u32) -> Self {
        let mut header = self.header();
        header.nSamplesPerSec = rate_hz;
        header.nAvgBytesPerSec = rate_hz * u32::from(header.nBlockAlign);
        let mut bytes = self.0.clone();
        // SAFETY: `bytes` holds at least a whole header.
        unsafe {
            bytes
                .as_mut_ptr()
                .cast::<WAVEFORMATEX>()
                .write_unaligned(header)
        };
        Self(bytes)
    }

    fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn from_hex(text: &str) -> Option<Self> {
        if !text.len().is_multiple_of(2) || text.len() / 2 < size_of::<WAVEFORMATEX>() {
            return None;
        }
        let bytes = (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        let format = Self(bytes);
        let expected = size_of::<WAVEFORMATEX>() + usize::from(format.header().cbSize);
        (format.0.len() == expected).then_some(format)
    }
}

/// The endpoint and the formats it had before Adaptive changed it.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedFormats {
    device_id: String,
    device_format: String,
    mix_format: String,
}

#[derive(Clone)]
struct OriginalFormats {
    device_id: String,
    device_format: WaveFormat,
    mix_format: WaveFormat,
}

impl OriginalFormats {
    fn from_saved(saved: SavedFormats) -> Option<Self> {
        Some(Self {
            device_id: saved.device_id,
            device_format: WaveFormat::from_hex(&saved.device_format)?,
            mix_format: WaveFormat::from_hex(&saved.mix_format)?,
        })
    }

    fn to_saved(&self) -> SavedFormats {
        SavedFormats {
            device_id: self.device_id.clone(),
            device_format: self.device_format.to_hex(),
            mix_format: self.mix_format.to_hex(),
        }
    }
}

fn policy_config() -> windows::core::Result<IPolicyConfig> {
    // SAFETY: COM is initialized by the caller's ComScope.
    unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
}

fn device_formats(
    policy: &IPolicyConfig,
    device_id: &str,
) -> Result<(WaveFormat, WaveFormat), String> {
    let id = HSTRING::from(device_id);
    let (mut device, mut mix) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: both out pointers are locals; the returned formats are taken
    // (copied and freed) right away.
    unsafe {
        policy
            .GetDeviceFormat(PCWSTR(id.as_ptr()), 0, &mut device)
            .ok()
            .map_err(|error| format!("The output device format could not be read: {error}"))?;
        let device = WaveFormat::take(device);
        policy
            .GetMixFormat(PCWSTR(id.as_ptr()), &mut mix)
            .ok()
            .map_err(|error| format!("The output mix format could not be read: {error}"))?;
        let mix = WaveFormat::take(mix);
        match (device, mix) {
            (Some(device), Some(mix)) => Ok((device, mix)),
            _ => Err("The output device reported no format.".to_owned()),
        }
    }
}

fn set_device_formats(
    policy: &IPolicyConfig,
    device_id: &str,
    device_format: &WaveFormat,
    mix_format: &WaveFormat,
) -> Result<(), String> {
    let id = HSTRING::from(device_id);
    // SAFETY: both formats are whole and outlive the call.
    unsafe {
        policy
            .SetDeviceFormat(
                PCWSTR(id.as_ptr()),
                device_format.as_ptr(),
                mix_format.as_ptr(),
            )
            .ok()
    }
    .map_err(|error| format!("The output device format could not be changed: {error}"))
}

/// Whether the endpoint's hardware accepts `format`; the Sound control panel
/// offers only such formats.
fn device_supports(device_id: &str, format: &WaveFormat) -> Result<bool, String> {
    let supports = || -> windows::core::Result<bool> {
        // SAFETY: COM is initialized by the caller's ComScope; `format` is whole.
        unsafe {
            let device = enumerator()?.GetDevice(&HSTRING::from(device_id))?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            Ok(
                client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, format.as_ptr(), None)
                    == S_OK,
            )
        }
    };
    supports().map_err(|error| format!("The output device formats could not be checked: {error}"))
}

fn is_device_active(device_id: &str) -> bool {
    let state = || -> windows::core::Result<bool> {
        // SAFETY: COM is initialized by the caller's ComScope.
        unsafe {
            let device = enumerator()?.GetDevice(&HSTRING::from(device_id))?;
            Ok(device.GetState()? == DEVICE_STATE_ACTIVE)
        }
    };
    state().unwrap_or(false)
}

/// Forces the default render endpoint's shared-mode rate through
/// `IPolicyConfig`. Each changed endpoint's original formats live in
/// `saved_path` from its first change until they are restored.
///
/// An endpoint that is absent when its formats should come back stays saved,
/// and every start restores the saved endpoints that are present, whether or
/// not Adaptive's cleanup marker asks for it: reconnecting the endpoint later
/// still gets its format back, and Adaptive on another endpoint adds an entry
/// instead of replacing it.
pub(crate) struct PolicyConfigRateAdapter {
    saved_path: PathBuf,
    lock: Mutex<()>,
}

impl PolicyConfigRateAdapter {
    pub(crate) fn new(saved_path: PathBuf) -> Self {
        let adapter = Self {
            saved_path,
            lock: Mutex::new(()),
        };
        if let Err(error) = adapter.restore_saved() {
            eprintln!("Adaptive could not restore a saved output format at start: {error}");
        }
        adapter
    }

    fn load(&self) -> Result<Vec<OriginalFormats>, String> {
        let text = match fs::read_to_string(&self.saved_path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("{}: {error}", self.saved_path.display())),
        };
        let saved: Vec<SavedFormats> = serde_json::from_str(&text)
            .map_err(|error| format!("{}: {error}", self.saved_path.display()))?;
        saved
            .into_iter()
            .map(OriginalFormats::from_saved)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("{}: invalid saved format", self.saved_path.display()))
    }

    /// Writes the endpoints still waiting for their formats; none removes the file.
    fn store(&self, originals: &[OriginalFormats]) -> Result<(), String> {
        if originals.is_empty() {
            return match fs::remove_file(&self.saved_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("{}: {error}", self.saved_path.display())),
            };
        }
        if let Some(parent) = self.saved_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let saved: Vec<SavedFormats> = originals.iter().map(OriginalFormats::to_saved).collect();
        let text = serde_json::to_string(&saved).map_err(|error| error.to_string())?;
        fs::write(&self.saved_path, text).map_err(|error| {
            format!(
                "Adaptive could not save the original output format to {}: {error}",
                self.saved_path.display()
            )
        })
    }

    /// Restores every saved endpoint that is present and saves the rest
    /// again: an absent endpoint, or one whose restore failed (the first such
    /// error is returned). `keep` is saved with them untouched, so an entry
    /// the caller still needs never leaves the file.
    fn restore_present(
        &self,
        policy: &IPolicyConfig,
        originals: Vec<OriginalFormats>,
        keep: &[OriginalFormats],
    ) -> Result<Vec<OriginalFormats>, String> {
        let mut pending = Vec::new();
        let mut first_error = None;
        for original in originals {
            if !is_device_active(&original.device_id) {
                eprintln!(
                    "Adaptive keeps the original format of an absent output device until it returns."
                );
                pending.push(original);
                continue;
            }
            eprintln!("Adaptive restoring the original output format.");
            if let Err(error) = set_device_formats_if_changed(
                policy,
                &original.device_id,
                &original.device_format,
                &original.mix_format,
            ) {
                first_error.get_or_insert(error);
                pending.push(original);
            }
        }
        let mut stored = pending.clone();
        stored.extend_from_slice(keep);
        self.store(&stored)?;
        match first_error {
            Some(error) => Err(error),
            None => Ok(pending),
        }
    }

    /// Puts back the formats of every saved endpoint that is present.
    fn restore_saved(&self) -> Result<(), String> {
        let originals = self.load()?;
        if originals.is_empty() {
            return Ok(());
        }
        let _com = ComScope::enter()?;
        let policy = policy_config().map_err(|_| UNAVAILABLE_MESSAGE.to_owned())?;
        self.restore_present(&policy, originals, &[]).map(|_| ())
    }

    fn force(&self, policy: &IPolicyConfig, rate_hz: u32) -> Result<(), String> {
        let device = default_render_device(&enumerator().map_err(|e| e.to_string())?)
            .map_err(|error| format!("Adaptive could not find the default output: {error}"))?;
        let device_id = endpoint_id(&device).map_err(|error| error.to_string())?;
        let (current, others): (Vec<_>, Vec<_>) = self
            .load()?
            .into_iter()
            .partition(|original| original.device_id == device_id);
        // The default output changed: the previous ones get their formats
        // back, while this one's saved original stays in the file.
        let mut saved = self.restore_present(policy, others, &current)?;
        let original = match current.into_iter().next() {
            Some(original) => original,
            None => {
                let (device_format, mix_format) = device_formats(policy, &device_id)?;
                OriginalFormats {
                    device_id: device_id.clone(),
                    device_format,
                    mix_format,
                }
            }
        };
        let mut device_format = original.device_format.with_sample_rate(rate_hz);
        let mut mix_format = original.mix_format.with_sample_rate(rate_hz);
        let (original_device, original_mix) =
            (original.device_format.clone(), original.mix_format.clone());
        saved.push(original);
        // Saved before anything changes, so a crash can be undone.
        self.store(&saved)?;
        if !device_supports(&device_id, &device_format)? {
            // The hardware cannot run at this rate: play through the original
            // format, which Windows resamples to, rather than failing the Track.
            eprintln!(
                "Adaptive kept the original output format: the device does not support {rate_hz} Hz."
            );
            device_format = original_device;
            mix_format = original_mix;
        }
        set_device_formats_if_changed(policy, &device_id, &device_format, &mix_format)
    }
}

/// Writes the formats only when the endpoint does not have them already.
/// Windows restarts every stream on the endpoint for each write, the same
/// format included, so a repeated rate (the next Track at the same rate, or
/// the decoder confirming the rate a load already set) must not write again.
fn set_device_formats_if_changed(
    policy: &IPolicyConfig,
    device_id: &str,
    device_format: &WaveFormat,
    mix_format: &WaveFormat,
) -> Result<(), String> {
    let (current_device, current_mix) = device_formats(policy, device_id)?;
    if &current_device == device_format && &current_mix == mix_format {
        return Ok(());
    }
    eprintln!(
        "Adaptive output format: device {} -> {} Hz, mix {} -> {} Hz",
        current_device.sample_rate_hz(),
        device_format.sample_rate_hz(),
        current_mix.sample_rate_hz(),
        mix_format.sample_rate_hz(),
    );
    wait_for_format_change_interval();
    // No stream of ours stays open on the endpoint while Windows restarts it.
    let closed = crate::playback::set_mpv_audio_enabled(false);
    if let Err(error) = &closed {
        eprintln!("Adaptive could not close mpv's audio before the format change: {error}");
    } else {
        thread::sleep(STREAM_CLOSE_SETTLE);
    }
    let result = set_device_formats(policy, device_id, device_format, mix_format)
        .and_then(|()| verify_device_formats(policy, device_id, device_format, mix_format));
    record_format_change();
    if closed.is_ok()
        && let Err(error) = crate::playback::set_mpv_audio_enabled(true)
    {
        eprintln!("Adaptive could not reopen mpv's audio after the format change: {error}");
    }
    result
}

/// Reads the formats back: Windows may accept a write and keep another format.
fn verify_device_formats(
    policy: &IPolicyConfig,
    device_id: &str,
    device_format: &WaveFormat,
    mix_format: &WaveFormat,
) -> Result<(), String> {
    let (current_device, current_mix) = device_formats(policy, device_id)?;
    if &current_device == device_format && &current_mix == mix_format {
        return Ok(());
    }
    Err(format!(
        "Windows kept the output at {} Hz instead of {} Hz.",
        current_device.sample_rate_hz(),
        device_format.sample_rate_hz()
    ))
}

/// Each write restarts the endpoint's audio engine; writes closer together
/// than this (quick Track or Output Mode changes) are spaced out instead of
/// restarting it back to back.
const MIN_FORMAT_CHANGE_INTERVAL: Duration = Duration::from_secs(1);
/// Lets mpv's WASAPI stream close before the endpoint restarts.
const STREAM_CLOSE_SETTLE: Duration = Duration::from_millis(50);
static LAST_FORMAT_CHANGE: Mutex<Option<Instant>> = Mutex::new(None);

fn wait_for_format_change_interval() {
    let last = LAST_FORMAT_CHANGE.lock().ok().and_then(|last| *last);
    if let Some(remaining) =
        last.and_then(|last| MIN_FORMAT_CHANGE_INTERVAL.checked_sub(last.elapsed()))
    {
        thread::sleep(remaining);
    }
}

fn record_format_change() {
    if let Ok(mut last) = LAST_FORMAT_CHANGE.lock() {
        *last = Some(Instant::now());
    }
}

impl PipeWireRateAdapter for PolicyConfigRateAdapter {
    /// The rate Adaptive left on a saved endpoint that is present.
    fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
        let _lock = self
            .lock
            .lock()
            .map_err(|_| "Adaptive state is unavailable.")?;
        let Some(original) = self
            .load()?
            .into_iter()
            .find(|original| is_device_active(&original.device_id))
        else {
            return Ok(None);
        };
        let _com = ComScope::enter()?;
        let policy = policy_config().map_err(|error| error.to_string())?;
        let (device_format, _) = device_formats(&policy, &original.device_id)?;
        Ok(Some(device_format.sample_rate_hz()))
    }

    /// Clearing the rate restores every saved endpoint that is present; an
    /// absent one stays saved for a later start rather than failing the caller.
    fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
        let _lock = self
            .lock
            .lock()
            .map_err(|_| "Adaptive state is unavailable.")?;
        match rate_hz {
            Some(rate_hz) => {
                let _com = ComScope::enter()?;
                let policy = policy_config().map_err(|_| UNAVAILABLE_MESSAGE.to_owned())?;
                self.force(&policy, rate_hz)
            }
            None => self.restore_saved(),
        }
    }

    fn ensure_available(&self) -> Result<(), String> {
        let _com = ComScope::enter()?;
        policy_config()
            .map(|_| ())
            .map_err(|_| UNAVAILABLE_MESSAGE.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::WaveFormat;
    use windows::Win32::Media::Audio::WAVEFORMATEX;

    fn stereo_24_bit(rate_hz: u32) -> WaveFormat {
        let header = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 2,
            nSamplesPerSec: rate_hz,
            nAvgBytesPerSec: rate_hz * 6,
            nBlockAlign: 6,
            wBitsPerSample: 24,
            cbSize: 2,
        };
        let mut bytes = vec![0_u8; size_of::<WAVEFORMATEX>() + 2];
        // SAFETY: `bytes` holds a whole header.
        unsafe {
            bytes
                .as_mut_ptr()
                .cast::<WAVEFORMATEX>()
                .write_unaligned(header)
        };
        bytes[size_of::<WAVEFORMATEX>()..].copy_from_slice(&[0xab, 0xcd]);
        WaveFormat(bytes)
    }

    #[test]
    fn a_new_rate_keeps_the_format_and_moves_the_byte_rate() {
        let changed = stereo_24_bit(48_000).with_sample_rate(44_100);

        assert_eq!(changed.sample_rate_hz(), 44_100);
        assert_eq!({ changed.header().nAvgBytesPerSec }, 44_100 * 6);
        assert_eq!({ changed.header().wBitsPerSample }, 24);
        assert_eq!(&changed.0[size_of::<WAVEFORMATEX>()..], &[0xab, 0xcd]);
    }

    #[test]
    #[ignore = "reads the real default output's formats through IPolicyConfig"]
    fn real_policy_config_reads_the_default_output_formats() {
        let _com = crate::wasapi::ComScope::enter().expect("COM");
        let policy = super::policy_config().expect("IPolicyConfig");
        let device = crate::wasapi::default_render_device(&crate::wasapi::enumerator().unwrap())
            .expect("default output");
        let id = crate::wasapi::endpoint_id(&device).expect("endpoint id");

        let (device_format, mix_format) = super::device_formats(&policy, &id).expect("formats");
        eprintln!(
            "default output: device {} Hz/{} bit, mix {} Hz/{} bit",
            device_format.sample_rate_hz(),
            { device_format.header().wBitsPerSample },
            mix_format.sample_rate_hz(),
            { mix_format.header().wBitsPerSample },
        );
        assert!(device_format.sample_rate_hz() >= 8_000);
        assert!(mix_format.sample_rate_hz() >= 8_000);
        assert!(super::device_supports(&id, &device_format).expect("support check"));
    }

    #[test]
    fn saved_formats_round_trip_through_hex() {
        let format = stereo_24_bit(96_000);

        assert_eq!(WaveFormat::from_hex(&format.to_hex()), Some(format));
        assert_eq!(WaveFormat::from_hex("00ff"), None);
    }
}
