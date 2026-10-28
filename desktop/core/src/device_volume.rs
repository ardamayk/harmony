// The pactl/amixer routes below are Linux's; Windows uses `wasapi`.
#![cfg_attr(windows, allow(dead_code))]

use std::process::Command;

/// Where a bit-perfect Output Mode keeps the listener's attenuation instead of
/// mpv's software volume: the PipeWire default sink (Adaptive), or the raw
/// ALSA card's own playback volume control (Exclusive).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VolumeRoute {
    SystemSink,
    AlsaCard(u32),
}

impl VolumeRoute {
    #[cfg(not(windows))]
    pub(crate) fn for_alsa_device(device_id: &str) -> Option<Self> {
        let card = device_id.strip_prefix("hw:")?.split(',').next()?;
        card.parse().ok().map(Self::AlsaCard)
    }

    /// Exclusive opens the default WASAPI endpoint, whose own volume is the
    /// system output's.
    #[cfg(windows)]
    pub(crate) fn for_alsa_device(_device_id: &str) -> Option<Self> {
        Some(Self::SystemSink)
    }
}

#[cfg(unix)]
pub(crate) type PlatformDeviceVolume = CommandDeviceVolume;
#[cfg(windows)]
pub(crate) type PlatformDeviceVolume = crate::wasapi::WasapiDeviceVolume;

pub(crate) trait DeviceVolume: Send + Sync {
    /// Changes the route's volume by `delta_db` and returns the change the
    /// device actually made, which is smaller when it hits its limit.
    fn change_db(&self, route: VolumeRoute, delta_db: f64) -> Result<f64, String>;
}

pub(crate) struct CommandDeviceVolume;

impl DeviceVolume for CommandDeviceVolume {
    fn change_db(&self, route: VolumeRoute, delta_db: f64) -> Result<f64, String> {
        match route {
            VolumeRoute::SystemSink => change_system_sink(delta_db),
            VolumeRoute::AlsaCard(card) => change_alsa_card(card, delta_db),
        }
    }
}

fn change_system_sink(delta_db: f64) -> Result<f64, String> {
    change_sink("@DEFAULT_SINK@", delta_db)
}

/// The default PipeWire sink's name, which a hand-off record names so its
/// undo reaches that sink even after the default changed.
pub(crate) fn default_sink_name() -> Result<String, String> {
    let name = run("pactl", &["get-default-sink"])?.trim().to_owned();
    if name.is_empty() || name == "@DEFAULT_SINK@" {
        return Err("The system output could not be resolved.".to_owned());
    }
    Ok(name)
}

/// Changes a named sink's volume, to undo a recorded hand-off.
pub(crate) fn change_named_sink_db(sink: &str, delta_db: f64) -> Result<f64, String> {
    change_sink(sink, delta_db)
}

/// PipeWire splits a sink volume between the hardware control and software
/// itself, so the whole change always lands.
fn change_sink(sink: &str, delta_db: f64) -> Result<f64, String> {
    let output = run("pactl", &["get-sink-volume", sink])?;
    let raw = parse_pactl_raw_volume(&output)
        .ok_or_else(|| "The system output volume could not be read.".to_owned())?;
    // shortcut: one value for every channel resets a non-centre balance; read
    // each channel when balance matters.
    let next = scale_pactl_raw_volume(raw, delta_db).to_string();
    run("pactl", &["set-sink-volume", sink, &next])?;
    Ok(delta_db)
}

fn change_alsa_card(card: u32, delta_db: f64) -> Result<f64, String> {
    let card = card.to_string();
    let controls = run("amixer", &["-c", &card, "scontents"])?;
    let control = first_playback_volume_control(&controls)
        .ok_or_else(|| "The output device has no hardware volume control.".to_owned())?;
    let read = || -> Result<f64, String> {
        let output = run("amixer", &["-c", &card, "sget", &control])?;
        parse_amixer_db(&output)
            .ok_or_else(|| "The output device volume could not be read.".to_owned())
    };
    let before = read()?;
    let step = format!(
        "{:.2}dB{}",
        delta_db.abs(),
        if delta_db < 0.0 { "-" } else { "+" }
    );
    run("amixer", &["-c", &card, "-q", "sset", &control, &step])?;
    Ok(read()? - before)
}

fn run(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("{program} could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `pactl` raw volumes are cubic: amplitude = (raw / 65536)^3.
fn scale_pactl_raw_volume(raw: u32, delta_db: f64) -> u32 {
    (f64::from(raw) * 10f64.powf(delta_db / 60.0)).round() as u32
}

fn parse_pactl_raw_volume(output: &str) -> Option<u32> {
    let (_, rest) = output.split_once(':')?;
    let (_, rest) = rest.split_once(':')?;
    rest.split_whitespace().next()?.parse().ok()
}

fn first_playback_volume_control(scontents: &str) -> Option<String> {
    let mut name = None;
    for line in scontents.lines() {
        if let Some(rest) = line.strip_prefix("Simple mixer control '") {
            name = rest.split_once('\'').map(|(name, _)| name.to_owned());
        } else if line.trim_start().starts_with("Capabilities:")
            && line
                .split_whitespace()
                .any(|capability| capability == "pvolume")
            && name.is_some()
        {
            return name;
        }
    }
    None
}

fn parse_amixer_db(output: &str) -> Option<f64> {
    output
        .lines()
        .find(|line| line.contains("Playback") && line.contains("dB]"))?
        .rsplit('[')
        .find(|part| part.ends_with("dB]"))?
        .trim_end_matches("dB]")
        .parse()
        .ok()
}

/// Rounds an attenuation to the 0.5 dB grid USB Audio volume controls use, so
/// PipeWire does not add a software remainder on top of the hardware step.
pub(crate) fn hardware_step_db(software_volume: f64) -> f64 {
    (20.0 * software_volume.log10() * 2.0).round() / 2.0
}

#[cfg(test)]
mod tests {
    use super::{
        VolumeRoute, first_playback_volume_control, hardware_step_db, parse_amixer_db,
        parse_pactl_raw_volume, scale_pactl_raw_volume,
    };

    #[cfg_attr(windows, ignore = "asserts Linux raw ALSA device ids")]
    #[test]
    fn parses_the_tools_this_route_reads() {
        assert_eq!(
            parse_pactl_raw_volume(
                "Volume: front-left: 11796 /  18% / -44.68 dB,   front-right: 11796 /  18% / -44.68 dB\n        balance 0.00\n"
            ),
            Some(11796)
        );
        let scontents = "Simple mixer control 'Mic',0\n  Capabilities: cvolume\nSimple mixer control 'PCM',0\n  Capabilities: pvolume\n  Playback channels: Front Left - Front Right\n";
        assert_eq!(
            first_playback_volume_control(scontents).as_deref(),
            Some("PCM")
        );
        assert_eq!(
            parse_amixer_db(
                "Simple mixer control 'PCM',0\n  Front Left: Playback 31 [26%] [-44.50dB]\n"
            ),
            Some(-44.5)
        );
        assert_eq!(
            VolumeRoute::for_alsa_device("hw:3,0"),
            Some(VolumeRoute::AlsaCard(3))
        );
    }

    #[test]
    fn cubic_raw_volume_moves_by_the_requested_decibels() {
        let scaled = scale_pactl_raw_volume(11796, -3.5);
        let delta = 60.0 * (f64::from(scaled) / 11796.0).log10();
        assert!((delta + 3.5).abs() < 0.01, "{delta}");
        assert_eq!(hardware_step_db(0.68), -3.5);
        assert_eq!(hardware_step_db(0.4), -8.0);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod real_device_tests {
    use super::{CommandDeviceVolume, DeviceVolume, VolumeRoute};

    #[test]
    #[ignore = "changes the real default output volume and restores it"]
    fn real_system_sink_moves_by_the_handed_off_step_and_back() {
        let volume = CommandDeviceVolume;
        assert_eq!(volume.change_db(VolumeRoute::SystemSink, -3.5), Ok(-3.5));
        assert_eq!(volume.change_db(VolumeRoute::SystemSink, 3.5), Ok(3.5));
    }
}
