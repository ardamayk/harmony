use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const GUARD_READY_MESSAGE: &str = "READY";
const DEVICE_RELEASE_TIMEOUT: Duration = Duration::from_secs(1);
const DEVICE_RELEASE_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const CARD_GUARD_SCRIPT: &str = r#"
set -u
card=$1
profile=$2
sink=$3
pactl_path=$4
restore() {
    "$pactl_path" set-card-profile "$card" "$profile" || return 1
    attempt=0
    while ! "$pactl_path" set-default-sink "$sink"; do
        attempt=$((attempt + 1))
        [ "$attempt" -ge 100 ] && return 1
        sleep 0.01
    done
}
trap restore EXIT
trap 'exit 0' HUP INT TERM
"$pactl_path" set-card-profile "$card" off || exit 1
printf 'READY\n'
while IFS= read -r _; do :; done
trap - EXIT
restore
"#;

pub(crate) trait ExclusiveOutputCoordinator: Send + Sync {
    fn acquire(&self, device_id: &str) -> Result<(), String>;
    fn release(&self) -> Result<(), String>;
}

trait AudioCardProfileLease: Send {
    fn release(&mut self) -> Result<(), String>;
}

trait AudioCardProfileLeaseLauncher: Send + Sync {
    fn acquire_for_default_sink(
        &self,
        device_id: &str,
    ) -> Result<Box<dyn AudioCardProfileLease>, String>;
}

pub(crate) struct PipeWireExclusiveOutputCoordinator {
    launcher: Box<dyn AudioCardProfileLeaseLauncher>,
    active: Mutex<Option<Box<dyn AudioCardProfileLease>>>,
}

impl PipeWireExclusiveOutputCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            launcher: Box::new(CommandAudioCardProfileLeaseLauncher::new()),
            active: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_launcher(launcher: Box<dyn AudioCardProfileLeaseLauncher>) -> Self {
        Self {
            launcher,
            active: Mutex::new(None),
        }
    }
}

impl ExclusiveOutputCoordinator for PipeWireExclusiveOutputCoordinator {
    fn acquire(&self, device_id: &str) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Exclusive Output ownership state is unavailable.".to_owned())?;
        if active.is_some() {
            return Ok(());
        }
        *active = Some(self.launcher.acquire_for_default_sink(device_id)?);
        Ok(())
    }

    fn release(&self) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Exclusive Output ownership state is unavailable.".to_owned())?;
        let Some(mut lease) = active.take() else {
            return Ok(());
        };
        if let Err(error) = lease.release() {
            *active = Some(lease);
            return Err(error);
        }
        Ok(())
    }
}

impl Default for PipeWireExclusiveOutputCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

struct CommandAudioCardProfileLeaseLauncher {
    pactl_path: PathBuf,
}

impl CommandAudioCardProfileLeaseLauncher {
    fn new() -> Self {
        Self {
            pactl_path: PathBuf::from("pactl"),
        }
    }

    fn default_output(&self) -> Result<SystemOutputProfile, String> {
        let sink = self.default_sink()?;
        let sinks = parse_json_output(
            self.pactl(&["--format=json", "list", "sinks"]),
            "inspect the active system output",
        )?;
        let sink_data = find_named_object(&sinks, &sink, "active system output")?;
        let card = sink_data
            .pointer("/properties/device.name")
            .and_then(Value::as_str)
            .filter(|card| !card.is_empty())
            .ok_or_else(|| "Exclusive Output could not resolve the active audio card.".to_owned())?
            .to_owned();
        let cards = parse_json_output(
            self.pactl(&["--format=json", "list", "cards"]),
            "inspect the active audio card",
        )?;
        let card_data = find_named_object(&cards, &card, "active audio card")?;
        let profile = card_data
            .get("active_profile")
            .and_then(Value::as_str)
            .filter(|profile| !profile.is_empty() && *profile != "off")
            .ok_or_else(|| {
                "Exclusive Output could not resolve the active card profile.".to_owned()
            })?
            .to_owned();
        Ok(SystemOutputProfile {
            sink,
            card,
            profile,
        })
    }

    fn default_sink(&self) -> Result<String, String> {
        let output = self.pactl(&["get-default-sink"])?;
        let sink = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if sink.is_empty() || sink == "@DEFAULT_SINK@" {
            return Err("Exclusive Output could not resolve the active system output.".to_owned());
        }
        Ok(sink)
    }

    fn pactl(&self, arguments: &[&str]) -> Result<Output, String> {
        let output = Command::new(&self.pactl_path)
            .args(arguments)
            .output()
            .map_err(|error| format!("Exclusive Output could not run pactl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Exclusive Output pactl command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output)
    }
}

impl AudioCardProfileLeaseLauncher for CommandAudioCardProfileLeaseLauncher {
    fn acquire_for_default_sink(
        &self,
        device_id: &str,
    ) -> Result<Box<dyn AudioCardProfileLease>, String> {
        let output = self.default_output()?;
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(CARD_GUARD_SCRIPT)
            .arg("earthly-audio-exclusive-output-guard")
            .arg(&output.card)
            .arg(&output.profile)
            .arg(&output.sink)
            .arg(&self.pactl_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Exclusive Output guard could not start: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Exclusive Output guard input is unavailable.".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Exclusive Output guard output is unavailable.".to_owned())?;
        let mut response = String::new();
        BufReader::new(stdout)
            .read_line(&mut response)
            .map_err(|error| format!("Exclusive Output guard did not respond: {error}"))?;
        if response.trim() != GUARD_READY_MESSAGE {
            return Err(guard_start_error(child, stdin));
        }
        let lease = CommandAudioCardProfileLease {
            output,
            pactl_path: self.pactl_path.clone(),
            child: Some(child),
            stdin: Some(stdin),
            is_released: false,
        };
        wait_for_raw_alsa_device(device_id)?;
        Ok(Box::new(lease))
    }
}

fn parse_json_output(output: Result<Output, String>, context: &str) -> Result<Value, String> {
    let output = output?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Exclusive Output could not {context}: {error}"))
}

fn find_named_object<'a>(
    collection: &'a Value,
    name: &str,
    description: &str,
) -> Result<&'a Value, String> {
    collection
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| format!("Exclusive Output could not find the {description} '{name}'."))
}

fn wait_for_raw_alsa_device(device_id: &str) -> Result<(), String> {
    let device_path = raw_alsa_playback_path(device_id)?;
    let deadline = Instant::now() + DEVICE_RELEASE_TIMEOUT;
    loop {
        let status = Command::new("fuser")
            .arg("-s")
            .arg(&device_path)
            .status()
            .map_err(|error| {
                format!(
                    "Exclusive Output could not inspect raw ALSA device '{}': {error}",
                    device_path.display()
                )
            })?;
        if status.code() == Some(1) {
            return Ok(());
        }
        if !status.success() {
            return Err(format!(
                "Exclusive Output could not inspect raw ALSA device '{}'.",
                device_path.display()
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Exclusive Output timed out while releasing raw ALSA device '{}'.",
                device_path.display()
            ));
        }
        thread::sleep(DEVICE_RELEASE_RETRY_INTERVAL);
    }
}

fn raw_alsa_playback_path(device_id: &str) -> Result<PathBuf, String> {
    let hardware = device_id
        .strip_prefix("hw:")
        .ok_or_else(|| "Exclusive Output requires a raw ALSA hardware device.".to_owned())?;
    let (card, device) = hardware.split_once(',').ok_or_else(|| {
        "Exclusive Output received an invalid raw ALSA hardware device.".to_owned()
    })?;
    let card = card
        .parse::<u32>()
        .map_err(|_| "Exclusive Output received an invalid ALSA card index.".to_owned())?;
    let device = device
        .parse::<u32>()
        .map_err(|_| "Exclusive Output received an invalid ALSA device index.".to_owned())?;
    Ok(PathBuf::from(format!("/dev/snd/pcmC{card}D{device}p")))
}

fn guard_start_error(mut child: Child, stdin: ChildStdin) -> String {
    drop(stdin);
    let status = child.wait();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    match status {
        Ok(status) => format!(
            "Exclusive Output could not disable the active audio card ({status}): {}",
            stderr.trim()
        ),
        Err(error) => format!("Exclusive Output guard startup failed: {error}"),
    }
}

struct SystemOutputProfile {
    sink: String,
    card: String,
    profile: String,
}

struct CommandAudioCardProfileLease {
    output: SystemOutputProfile,
    pactl_path: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    is_released: bool,
}

impl CommandAudioCardProfileLease {
    fn finish(&mut self) -> Result<(), String> {
        if self.is_released {
            return Ok(());
        }
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            match child.wait() {
                Ok(status) if status.success() => {
                    self.is_released = true;
                    return Ok(());
                }
                Ok(status) => eprintln!(
                    "Exclusive Output guard could not restore audio card '{}' ({status}); retrying directly.",
                    self.output.card
                ),
                Err(error) => eprintln!(
                    "Exclusive Output guard status failed for audio card '{}': {error}; retrying directly.",
                    self.output.card
                ),
            }
        }
        restore_output_profile(&self.pactl_path, &self.output)?;
        self.is_released = true;
        Ok(())
    }
}

fn restore_output_profile(
    pactl_path: &PathBuf,
    output: &SystemOutputProfile,
) -> Result<(), String> {
    let profile_result = Command::new(pactl_path)
        .args(["set-card-profile", &output.card, &output.profile])
        .output()
        .map_err(|error| format!("Exclusive Output could not restore the audio card: {error}"))?;
    if !profile_result.status.success() {
        return Err(format!(
            "Exclusive Output could not restore audio card '{}': {}",
            output.card,
            String::from_utf8_lossy(&profile_result.stderr).trim()
        ));
    }
    let deadline = Instant::now() + DEVICE_RELEASE_TIMEOUT;
    loop {
        let sink_result = Command::new(pactl_path)
            .args(["set-default-sink", &output.sink])
            .output()
            .map_err(|error| {
                format!("Exclusive Output could not restore the default system output: {error}")
            })?;
        if sink_result.status.success() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Exclusive Output could not restore default system output '{}': {}",
                output.sink,
                String::from_utf8_lossy(&sink_result.stderr).trim()
            ));
        }
        thread::sleep(DEVICE_RELEASE_RETRY_INTERVAL);
    }
}

impl AudioCardProfileLease for CommandAudioCardProfileLease {
    fn release(&mut self) -> Result<(), String> {
        self.finish()
    }
}

impl Drop for CommandAudioCardProfileLease {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            eprintln!("{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioCardProfileLease, AudioCardProfileLeaseLauncher, ExclusiveOutputCoordinator,
        PipeWireExclusiveOutputCoordinator, raw_alsa_playback_path,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct RecordedLease {
        is_released: Arc<AtomicBool>,
    }

    impl AudioCardProfileLease for RecordedLease {
        fn release(&mut self) -> Result<(), String> {
            self.is_released.store(true, Ordering::Release);
            Ok(())
        }
    }

    struct RecordedLauncher {
        acquisitions: Arc<AtomicUsize>,
        is_released: Arc<AtomicBool>,
    }

    impl AudioCardProfileLeaseLauncher for RecordedLauncher {
        fn acquire_for_default_sink(
            &self,
            _device_id: &str,
        ) -> Result<Box<dyn AudioCardProfileLease>, String> {
            self.acquisitions.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(RecordedLease {
                is_released: self.is_released.clone(),
            }))
        }
    }

    struct RetryLease {
        release_attempts: Arc<AtomicUsize>,
    }

    impl AudioCardProfileLease for RetryLease {
        fn release(&mut self) -> Result<(), String> {
            if self.release_attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                return Err("temporary restore failure".to_owned());
            }
            Ok(())
        }
    }

    struct RetryLauncher {
        release_attempts: Arc<AtomicUsize>,
    }

    impl AudioCardProfileLeaseLauncher for RetryLauncher {
        fn acquire_for_default_sink(
            &self,
            _device_id: &str,
        ) -> Result<Box<dyn AudioCardProfileLease>, String> {
            Ok(Box::new(RetryLease {
                release_attempts: self.release_attempts.clone(),
            }))
        }
    }

    #[test]
    fn exclusive_output_disables_and_restores_the_active_audio_card_profile() {
        let acquisitions = Arc::new(AtomicUsize::new(0));
        let is_released = Arc::new(AtomicBool::new(false));
        let coordinator =
            PipeWireExclusiveOutputCoordinator::with_launcher(Box::new(RecordedLauncher {
                acquisitions: acquisitions.clone(),
                is_released: is_released.clone(),
            }));

        coordinator.acquire("hw:2,0").expect("disable active card");
        coordinator
            .acquire("hw:2,0")
            .expect("keep active card disabled");
        coordinator.release().expect("restore active card");

        assert_eq!(acquisitions.load(Ordering::Acquire), 1);
        assert!(is_released.load(Ordering::Acquire));
    }

    #[test]
    fn exclusive_output_retries_a_failed_profile_restore() {
        let release_attempts = Arc::new(AtomicUsize::new(0));
        let coordinator =
            PipeWireExclusiveOutputCoordinator::with_launcher(Box::new(RetryLauncher {
                release_attempts: release_attempts.clone(),
            }));
        coordinator.acquire("hw:2,0").expect("disable active card");

        assert!(coordinator.release().is_err());
        coordinator.release().expect("retry profile restore");

        assert_eq!(release_attempts.load(Ordering::Acquire), 2);
    }

    #[test]
    fn raw_alsa_device_maps_to_its_playback_node() {
        assert_eq!(
            raw_alsa_playback_path("hw:2,0").expect("raw playback path"),
            std::path::PathBuf::from("/dev/snd/pcmC2D0p")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "temporarily disables the active PipeWire audio card profile"]
    fn real_guard_disables_and_restores_the_active_audio_card_profile() {
        let coordinator = PipeWireExclusiveOutputCoordinator::new();

        coordinator.acquire("hw:2,0").expect("disable real card");
        coordinator.release().expect("restore real card");
    }
}
