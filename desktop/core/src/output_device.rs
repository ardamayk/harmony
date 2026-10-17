use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputDeviceError {
    EnumerationFailed(String),
    NotRawHardware,
    SelectedDeviceDisconnected(OutputDevice),
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActiveOutputError {
    QueryFailed(String),
    NoActiveOutput,
    NotAlsaBacked,
}

pub trait ActiveOutputResolver: Send + Sync {
    fn resolve(&self) -> Result<OutputDevice, ActiveOutputError>;
}

pub struct CommandPipeWireActiveOutputResolver {
    binary: PathBuf,
}

impl CommandPipeWireActiveOutputResolver {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("pw-dump"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for CommandPipeWireActiveOutputResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveOutputResolver for CommandPipeWireActiveOutputResolver {
    fn resolve(&self) -> Result<OutputDevice, ActiveOutputError> {
        let output = Command::new(&self.binary)
            .arg("-N")
            .output()
            .map_err(|error| ActiveOutputError::QueryFailed(error.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(ActiveOutputError::QueryFailed(stderr));
        }
        parse_active_raw_output(&output.stdout)
    }
}

pub trait AlsaOutputDeviceAdapter: Send + Sync {
    fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String>;
}

pub struct CommandAlsaOutputDeviceAdapter {
    binary: PathBuf,
}

impl CommandAlsaOutputDeviceAdapter {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("aplay"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for CommandAlsaOutputDeviceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AlsaOutputDeviceAdapter for CommandAlsaOutputDeviceAdapter {
    fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String> {
        let output = Command::new(&self.binary)
            .arg("-l")
            .output()
            .map_err(|error| format!("Failed to enumerate raw ALSA hardware: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(format!(
                "Raw ALSA hardware enumeration exited with {}: {stderr}",
                output.status
            ));
        }
        Ok(parse_aplay_hardware(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }
}

pub struct OutputDeviceCatalog {
    adapter: Box<dyn AlsaOutputDeviceAdapter>,
    devices: Vec<OutputDevice>,
    selected: Option<OutputDevice>,
}

impl OutputDeviceCatalog {
    pub fn new(adapter: Box<dyn AlsaOutputDeviceAdapter>) -> Self {
        Self {
            adapter,
            devices: Vec::new(),
            selected: None,
        }
    }

    pub fn refresh(&mut self) -> Result<Vec<OutputDevice>, OutputDeviceError> {
        self.devices = self
            .adapter
            .list_raw_devices()
            .map_err(OutputDeviceError::EnumerationFailed)?;
        if let Some(selected) = self.selected.as_ref()
            && !self.devices.iter().any(|device| device.id == selected.id)
        {
            return Err(OutputDeviceError::SelectedDeviceDisconnected(
                selected.clone(),
            ));
        }
        Ok(self.devices.clone())
    }

    pub fn select(&mut self, device_id: &str) -> Result<OutputDevice, OutputDeviceError> {
        if !is_raw_hardware_id(device_id) {
            return Err(OutputDeviceError::NotRawHardware);
        }
        self.devices = self
            .adapter
            .list_raw_devices()
            .map_err(OutputDeviceError::EnumerationFailed)?;
        let device = self
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
            .ok_or(OutputDeviceError::Unavailable)?;
        self.selected = Some(device.clone());
        Ok(device)
    }

    pub fn selected(&self) -> Option<&OutputDevice> {
        self.selected.as_ref()
    }

    pub fn devices(&self) -> &[OutputDevice] {
        &self.devices
    }
}

fn is_raw_hardware_id(device_id: &str) -> bool {
    let Some(components) = device_id.strip_prefix("hw:") else {
        return false;
    };
    let mut components = components.split(',');
    matches!(components.next(), Some(card) if card.parse::<u32>().is_ok())
        && matches!(components.next(), Some(device) if device.parse::<u32>().is_ok())
        && components.next().is_none()
}

fn parse_aplay_hardware(output: &str) -> Vec<OutputDevice> {
    output.lines().filter_map(parse_aplay_device_line).collect()
}

fn parse_aplay_device_line(line: &str) -> Option<OutputDevice> {
    let line = line.trim();
    let card = line.strip_prefix("card ")?;
    let (card_number, remainder) = card.split_once(':')?;
    let device = remainder.split(", device ").nth(1)?;
    let (device_number, description) = device.split_once(':')?;
    let card_number = card_number.trim().parse::<u32>().ok()?;
    let device_number = device_number.trim().parse::<u32>().ok()?;
    Some(OutputDevice {
        id: format!("hw:{card_number},{device_number}"),
        name: description.trim().to_owned(),
    })
}

fn parse_active_raw_output(input: &[u8]) -> Result<OutputDevice, ActiveOutputError> {
    let objects: Vec<Value> = serde_json::from_slice(input)
        .map_err(|error| ActiveOutputError::QueryFailed(error.to_string()))?;
    let sink_name = default_sink_name(&objects).ok_or(ActiveOutputError::NoActiveOutput)?;
    let sink = find_sink(&objects, &sink_name).ok_or(ActiveOutputError::NoActiveOutput)?;
    raw_output_from_sink(sink).ok_or(ActiveOutputError::NotAlsaBacked)
}

fn default_sink_name(objects: &[Value]) -> Option<String> {
    let metadata = objects.iter().find(|object| {
        object
            .pointer("/props/metadata.name")
            .and_then(Value::as_str)
            == Some("default")
    })?;
    ["default.audio.sink", "default.configured.audio.sink"]
        .into_iter()
        .find_map(|key| metadata_target_name(metadata, key))
}

fn metadata_target_name(metadata: &Value, key: &str) -> Option<String> {
    let value = metadata
        .get("metadata")?
        .as_array()?
        .iter()
        .find_map(|entry| {
            (entry.get("key")?.as_str()? == key)
                .then(|| entry.get("value"))
                .flatten()
        })?;
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        return Some(name.to_owned());
    }
    let encoded = value.as_str()?;
    serde_json::from_str::<Value>(encoded)
        .ok()?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

fn find_sink<'a>(objects: &'a [Value], name: &str) -> Option<&'a Value> {
    objects.iter().find(|object| {
        object
            .pointer("/info/props/media.class")
            .and_then(Value::as_str)
            == Some("Audio/Sink")
            && object
                .pointer("/info/props/node.name")
                .and_then(Value::as_str)
                == Some(name)
    })
}

fn raw_output_from_sink(sink: &Value) -> Option<OutputDevice> {
    let card = ["api.alsa.pcm.card", "api.alsa.card", "alsa.card"]
        .into_iter()
        .find_map(|key| sink_property_u32(sink, key))?;
    let device = ["api.alsa.pcm.device", "api.alsa.device", "alsa.device"]
        .into_iter()
        .find_map(|key| sink_property_u32(sink, key))?;
    Some(OutputDevice {
        id: format!("hw:{card},{device}"),
        name: sink_name(sink).unwrap_or_else(|| format!("ALSA hw:{card},{device}")),
    })
}

fn sink_property_u32(sink: &Value, key: &str) -> Option<u32> {
    let value = sink.pointer(&format!("/info/props/{key}"))?;
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn sink_name(sink: &Value) -> Option<String> {
    ["node.description", "node.nick", "node.name"]
        .into_iter()
        .find_map(|key| {
            sink.pointer(&format!("/info/props/{key}"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::{ActiveOutputError, OutputDevice, parse_active_raw_output};

    #[test]
    fn resolves_the_default_pipewire_sink_to_raw_alsa_hardware() {
        let fixture = br#"[
            {
                "type": "PipeWire:Interface:Metadata",
                "props": { "metadata.name": "default" },
                "metadata": [
                    { "key": "default.audio.sink", "value": "{\"name\":\"alsa_output.usb-dac\"}" }
                ]
            },
            {
                "type": "PipeWire:Interface:Node",
                "info": { "props": {
                    "media.class": "Audio/Sink",
                    "node.name": "alsa_output.usb-dac",
                    "node.description": "USB DAC",
                    "api.alsa.pcm.card": "2",
                    "alsa.device": 0
                } }
            }
        ]"#;

        assert_eq!(
            parse_active_raw_output(fixture),
            Ok(OutputDevice {
                id: "hw:2,0".to_owned(),
                name: "USB DAC".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_an_active_output_without_raw_alsa_hardware() {
        let fixture = br#"[
            {
                "type": "PipeWire:Interface:Metadata",
                "props": { "metadata.name": "default" },
                "metadata": [
                    { "key": "default.audio.sink", "value": "{\"name\":\"bluez_output.headset\"}" }
                ]
            },
            {
                "type": "PipeWire:Interface:Node",
                "info": { "props": {
                    "media.class": "Audio/Sink",
                    "node.name": "bluez_output.headset",
                    "node.description": "Bluetooth Headset"
                } }
            }
        ]"#;

        assert_eq!(
            parse_active_raw_output(fixture),
            Err(ActiveOutputError::NotAlsaBacked)
        );
    }

    #[test]
    fn reports_a_missing_default_output() {
        assert_eq!(
            parse_active_raw_output(br#"[]"#),
            Err(ActiveOutputError::NoActiveOutput)
        );
    }
}
