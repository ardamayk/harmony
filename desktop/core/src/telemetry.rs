use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFormatObservation {
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u16>,
}

impl AudioFormatObservation {
    pub fn unknown() -> Self {
        Self {
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceObservation {
    pub codec: Option<String>,
    pub bitrate_kbps: Option<u32>,
    pub format: AudioFormatObservation,
}

impl SourceObservation {
    pub fn from_playback_source(source: &Value) -> Self {
        let source_type = source.get("type").and_then(Value::as_str);
        let metadata = match source_type {
            Some("track") => source.get("track"),
            Some("radio-station") => source.get("station"),
            Some("catalog-preview") => source.get("result"),
            _ => None,
        };
        let is_track = source_type == Some("track");
        Self {
            codec: metadata
                .and_then(|value| value.get(if is_track { "format" } else { "codec" }))
                .and_then(Value::as_str)
                .map(str::to_uppercase),
            bitrate_kbps: observed_u32(metadata, if is_track { "bitrateKbps" } else { "bitrate" }),
            format: AudioFormatObservation {
                sample_rate_hz: is_track
                    .then(|| observed_u32(metadata, "sampleRateHz"))
                    .flatten(),
                bit_depth: is_track
                    .then(|| observed_u16(metadata, "bitDepth"))
                    .flatten(),
                channels: is_track
                    .then(|| observed_u16(metadata, "channels"))
                    .flatten(),
            },
        }
    }

    pub fn unknown() -> Self {
        Self {
            codec: None,
            bitrate_kbps: None,
            format: AudioFormatObservation::unknown(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedMpvProperties {
    pub pcm_format: Option<String>,
    pub format: AudioFormatObservation,
}

impl ObservedMpvProperties {
    pub fn unknown() -> Self {
        Self {
            pcm_format: None,
            format: AudioFormatObservation::unknown(),
        }
    }

    pub fn from_audio_params(value: &Value) -> Self {
        let pcm_format = value
            .get("format")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Self {
            format: AudioFormatObservation {
                sample_rate_hz: value.get("samplerate").and_then(value_u32),
                bit_depth: pcm_format.as_deref().and_then(observed_bit_depth),
                channels: value.get("channel-count").and_then(value_u16),
            },
            pcm_format,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipeWireObservation {
    pub graph_format: AudioFormatObservation,
    pub is_graph_resampling: Option<bool>,
    pub device_name: Option<String>,
    pub device_format: AudioFormatObservation,
    pub is_device_resampling: Option<bool>,
}

pub trait PipeWireObserver: Send + Sync {
    fn observe(&self) -> Result<Option<PipeWireObservation>, String>;
}

pub struct CommandPipeWireObserver {
    binary: PathBuf,
    target_node_name: Option<String>,
}

impl CommandPipeWireObserver {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("pw-dump"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self {
            binary,
            target_node_name: None,
        }
    }

    pub fn for_node(mut self, node_name: impl Into<String>) -> Self {
        self.target_node_name = Some(node_name.into());
        self
    }
}

impl Default for CommandPipeWireObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeWireObserver for CommandPipeWireObserver {
    fn observe(&self) -> Result<Option<PipeWireObservation>, String> {
        let output = match Command::new(&self.binary).arg("-N").output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "PipeWire observer {} is unavailable; telemetry will remain unknown: {error}",
                    self.binary.display()
                );
                return Ok(None);
            }
            Err(error) => {
                return Err(format!(
                    "Failed to run PipeWire observer {}: {error}",
                    self.binary.display()
                ));
            }
        };
        if !output.status.success() {
            let status = output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "terminated by signal".to_owned());
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(format!(
                "PipeWire observer {} exited with status {status}: {stderr}",
                self.binary.display()
            ));
        }
        parse_pw_dump_for_node(&output.stdout, self.target_node_name.as_deref())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemObservationKind {
    PipeWire,
    BrowserManaged,
    Bypassed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemObservation {
    pub kind: SystemObservationKind,
    pub format: AudioFormatObservation,
    pub is_resampling: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceObservation {
    pub name: Option<String>,
    pub format: AudioFormatObservation,
    pub is_resampling: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingObservation {
    pub profile: String,
    pub software_volume: Option<f64>,
    pub replay_gain_mode: String,
    pub effective_replay_gain_mode: String,
    pub is_equalizer_enabled: Option<bool>,
}

impl ProcessingObservation {
    pub fn direct() -> Self {
        Self {
            profile: "direct".to_owned(),
            software_volume: Some(1.0),
            replay_gain_mode: "off".to_owned(),
            effective_replay_gain_mode: "off".to_owned(),
            is_equalizer_enabled: Some(false),
        }
    }

    pub fn unknown() -> Self {
        Self {
            profile: "unknown".to_owned(),
            software_volume: None,
            replay_gain_mode: "unknown".to_owned(),
            effective_replay_gain_mode: "unknown".to_owned(),
            is_equalizer_enabled: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTelemetry {
    pub source: SourceObservation,
    pub decoder: ObservedMpvProperties,
    pub system: SystemObservation,
    pub device: DeviceObservation,
    pub processing: ProcessingObservation,
}

impl PlaybackTelemetry {
    pub fn native_system_output(
        source: SourceObservation,
        decoder: ObservedMpvProperties,
        pipewire: Option<PipeWireObservation>,
        processing: ProcessingObservation,
    ) -> Self {
        let (mut system, device) = observed_pipewire_path(pipewire);
        if system.is_resampling.is_none() {
            system.is_resampling = compare_sample_rates(&decoder.format, &system.format);
        }
        Self {
            source,
            decoder,
            system,
            device,
            processing,
        }
    }

    pub fn native_direct_alsa_output(
        source: SourceObservation,
        decoder: ObservedMpvProperties,
        device_name: &str,
        negotiated_format: Option<AudioFormatObservation>,
        processing: ProcessingObservation,
    ) -> Self {
        let device_format = negotiated_format.unwrap_or_else(AudioFormatObservation::unknown);
        let is_resampling = compare_sample_rates(&decoder.format, &device_format);
        Self {
            source,
            decoder,
            system: SystemObservation {
                kind: SystemObservationKind::Bypassed,
                format: AudioFormatObservation::unknown(),
                is_resampling: Some(false),
            },
            device: DeviceObservation {
                name: Some(device_name.to_owned()),
                format: device_format,
                is_resampling,
            },
            processing,
        }
    }
}

pub fn parse_pw_dump(input: &[u8]) -> Result<Option<PipeWireObservation>, String> {
    parse_pw_dump_for_node(input, None)
}

fn parse_pw_dump_for_node(
    input: &[u8],
    target_node_name: Option<&str>,
) -> Result<Option<PipeWireObservation>, String> {
    let objects: Vec<Value> = serde_json::from_slice(input)
        .map_err(|error| format!("Failed to parse pw-dump JSON: {error}"))?;
    let stream = find_earthly_audio_stream(&objects);
    let sink = target_node_name
        .and_then(|name| find_audio_sink(&objects, name))
        .or_else(|| stream.and_then(|stream| find_routed_sink(&objects, stream)));
    let device_format = sink
        .map(observed_node_format)
        .unwrap_or_else(AudioFormatObservation::unknown);
    let stream_format = stream
        .map(observed_node_format)
        .unwrap_or_else(AudioFormatObservation::unknown);
    let graph_format = AudioFormatObservation {
        sample_rate_hz: observed_graph_rate(&objects).or(stream_format.sample_rate_hz),
        bit_depth: stream_format.bit_depth,
        channels: stream_format.channels,
    };
    if graph_format == AudioFormatObservation::unknown() && sink.is_none() {
        return Ok(None);
    }
    Ok(Some(PipeWireObservation {
        is_device_resampling: compare_sample_rates(&graph_format, &device_format),
        graph_format,
        is_graph_resampling: None,
        device_name: sink.and_then(observed_device_name),
        device_format,
    }))
}

fn observed_pipewire_path(
    pipewire: Option<PipeWireObservation>,
) -> (SystemObservation, DeviceObservation) {
    let Some(pipewire) = pipewire else {
        return (
            SystemObservation {
                kind: SystemObservationKind::PipeWire,
                format: AudioFormatObservation::unknown(),
                is_resampling: None,
            },
            DeviceObservation {
                name: None,
                format: AudioFormatObservation::unknown(),
                is_resampling: None,
            },
        );
    };
    (
        SystemObservation {
            kind: SystemObservationKind::PipeWire,
            format: pipewire.graph_format,
            is_resampling: pipewire.is_graph_resampling,
        },
        DeviceObservation {
            name: pipewire.device_name,
            format: pipewire.device_format,
            is_resampling: pipewire.is_device_resampling,
        },
    )
}

fn observed_graph_rate(objects: &[Value]) -> Option<u32> {
    let settings = objects.iter().find(|object| {
        object
            .pointer("/props/metadata.name")
            .and_then(Value::as_str)
            == Some("settings")
    })?;
    let force_rate = metadata_u32(settings, "clock.force-rate").filter(|value| *value > 0);
    force_rate.or_else(|| metadata_u32(settings, "clock.rate"))
}

fn find_audio_sink<'a>(objects: &'a [Value], node_name: &str) -> Option<&'a Value> {
    objects.iter().find(|object| {
        object.get("type").and_then(Value::as_str) == Some("PipeWire:Interface:Node")
            && object
                .pointer("/info/props/media.class")
                .and_then(Value::as_str)
                == Some("Audio/Sink")
            && object
                .pointer("/info/props/node.name")
                .and_then(Value::as_str)
                == Some(node_name)
    })
}

fn find_earthly_audio_stream(objects: &[Value]) -> Option<&Value> {
    objects.iter().find(|object| {
        object
            .pointer("/info/props/media.class")
            .and_then(Value::as_str)
            == Some("Stream/Output/Audio")
            && ["application.name", "node.name"].into_iter().any(|key| {
                object
                    .pointer(&format!("/info/props/{key}"))
                    .and_then(Value::as_str)
                    == Some("Earthly Audio")
            })
    })
}

fn find_routed_sink<'a>(objects: &'a [Value], stream: &Value) -> Option<&'a Value> {
    let stream_id = stream.get("id").and_then(value_u32)?;
    let sink_id = objects
        .iter()
        .filter(|object| {
            object.get("type").and_then(Value::as_str) == Some("PipeWire:Interface:Link")
        })
        .find_map(|link| {
            (link.pointer("/info/output-node-id").and_then(value_u32) == Some(stream_id))
                .then(|| link.pointer("/info/input-node-id").and_then(value_u32))
                .flatten()
        })?;
    objects.iter().find(|object| {
        object.get("id").and_then(value_u32) == Some(sink_id)
            && object
                .pointer("/info/props/media.class")
                .and_then(Value::as_str)
                == Some("Audio/Sink")
    })
}

fn observed_node_format(node: &Value) -> AudioFormatObservation {
    let format = node
        .pointer("/info/params/Format")
        .and_then(Value::as_array)
        .and_then(|formats| formats.first());
    AudioFormatObservation {
        sample_rate_hz: format
            .and_then(|value| value.get("rate"))
            .and_then(value_u32),
        bit_depth: format
            .and_then(|value| value.get("format"))
            .and_then(Value::as_str)
            .and_then(observed_bit_depth),
        channels: format
            .and_then(|value| value.get("channels"))
            .and_then(value_u16),
    }
}

fn observed_device_name(node: &Value) -> Option<String> {
    ["node.description", "node.nick", "node.name"]
        .into_iter()
        .find_map(|key| {
            node.pointer(&format!("/info/props/{key}"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

fn observed_bit_depth(format: &str) -> Option<u16> {
    let normalized = format.to_uppercase();
    [8_u16, 16, 24, 32, 64].into_iter().find(|bits| {
        normalized.starts_with(&format!("S{bits}"))
            || normalized.starts_with(&format!("U{bits}"))
            || normalized.starts_with(&format!("F{bits}"))
    })
}

fn metadata_u32(object: &Value, key: &str) -> Option<u32> {
    metadata_value(object, key).and_then(value_u32)
}

fn metadata_value<'a>(object: &'a Value, key: &str) -> Option<&'a Value> {
    object
        .get("metadata")?
        .as_array()?
        .iter()
        .find_map(|entry| {
            (entry.get("key")?.as_str()? == key)
                .then(|| entry.get("value"))
                .flatten()
        })
}

fn compare_sample_rates(
    left: &AudioFormatObservation,
    right: &AudioFormatObservation,
) -> Option<bool> {
    Some(left.sample_rate_hz? != right.sample_rate_hz?)
}

fn value_u32(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|value| u32::try_from(value).ok())
}

fn value_u16(value: &Value) -> Option<u16> {
    value.as_u64().and_then(|value| u16::try_from(value).ok())
}

fn observed_u32(container: Option<&Value>, field: &str) -> Option<u32> {
    container?
        .get(field)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

fn observed_u16(container: Option<&Value>, field: &str) -> Option<u16> {
    container?
        .get(field)?
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
}
