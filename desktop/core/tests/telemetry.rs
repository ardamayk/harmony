// The PipeWire observer and its shell-script fixtures exist only on Unix.
#![cfg(unix)]

use earthly_audio_core::telemetry;
use earthly_audio_core::telemetry::{
    AudioFormatObservation, CommandPipeWireObserver, ObservedMpvProperties, PipeWireObservation,
    PipeWireObserver, PlaybackTelemetry, ProcessingObservation, SystemObservationKind,
    parse_pw_dump,
};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct ObservedPipeWire(Option<PipeWireObservation>);

impl PipeWireObserver for ObservedPipeWire {
    fn observe(&self) -> Result<Option<PipeWireObservation>, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn native_telemetry_keeps_source_decoder_system_device_and_processing_independent() {
    let source = telemetry::SourceObservation::from_playback_source(&json!({
        "type": "track",
        "track": {
            "format": "flac",
            "bitrateKbps": 1411,
            "sampleRateHz": 96000,
            "bitDepth": 24
        }
    }));
    let decoder = ObservedMpvProperties {
        pcm_format: Some("s24".to_owned()),
        format: format(96000, 24, 2),
    };
    let pipewire = PipeWireObservation {
        graph_format: format(48000, 24, 2),
        is_graph_resampling: Some(true),
        device_name: Some("USB DAC".to_owned()),
        device_format: format(48000, 32, 2),
        is_device_resampling: Some(false),
    };

    let pipewire = ObservedPipeWire(Some(pipewire))
        .observe()
        .expect("observe PipeWire");
    let observed = PlaybackTelemetry::native_system_output(
        source,
        decoder,
        pipewire,
        ProcessingObservation::direct(),
    );

    assert_eq!(observed.source.codec.as_deref(), Some("FLAC"));
    assert_eq!(observed.decoder.format.sample_rate_hz, Some(96000));
    assert_eq!(observed.system.kind, SystemObservationKind::PipeWire);
    assert_eq!(observed.system.format.sample_rate_hz, Some(48000));
    assert_eq!(observed.device.name.as_deref(), Some("USB DAC"));
    assert_eq!(observed.device.format.bit_depth, Some(32));
    assert_eq!(observed.processing.profile, "direct");
}

#[test]
fn direct_alsa_telemetry_reports_pipewire_bypass_without_inventing_device_format() {
    let decoder = ObservedMpvProperties::from_audio_params(
        &json!({ "format": "s24", "samplerate": 96000, "channel-count": 2 }),
    );

    let observed = PlaybackTelemetry::native_direct_alsa_output(
        telemetry::SourceObservation::unknown(),
        decoder,
        "USB DAC",
        None,
        ProcessingObservation::direct(),
    );

    assert_eq!(observed.system.kind, SystemObservationKind::Bypassed);
    assert_eq!(observed.system.format, AudioFormatObservation::unknown());
    assert_eq!(observed.device.name.as_deref(), Some("USB DAC"));
    assert_eq!(observed.device.format, AudioFormatObservation::unknown());
    assert_eq!(observed.device.is_resampling, None);
}

#[test]
fn radio_sources_publish_available_codec_and_bitrate_metadata() {
    let station = telemetry::SourceObservation::from_playback_source(&json!({
        "type": "radio-station",
        "station": { "codec": "aac+", "bitrate": 192 }
    }));
    let catalog = telemetry::SourceObservation::from_playback_source(&json!({
        "type": "catalog-preview",
        "result": { "codec": "ogg", "bitrate": 256 }
    }));

    assert_eq!(station.codec.as_deref(), Some("AAC+"));
    assert_eq!(station.bitrate_kbps, Some(192));
    assert_eq!(catalog.codec.as_deref(), Some("OGG"));
    assert_eq!(catalog.bitrate_kbps, Some(256));
    assert_eq!(catalog.format, AudioFormatObservation::unknown());
}

#[test]
fn external_tracks_publish_no_invented_source_metadata() {
    let external = telemetry::SourceObservation::from_playback_source(&json!({
        "type": "external-track",
        "externalTrack": { "id": "token.signature", "title": "Around the World", "extractor": "Soundcloud" }
    }));

    assert_eq!(external, telemetry::SourceObservation::unknown());
}

#[test]
fn missing_pipewire_evidence_remains_unknown() {
    let observed = PlaybackTelemetry::native_system_output(
        telemetry::SourceObservation::unknown(),
        ObservedMpvProperties::unknown(),
        None,
        ProcessingObservation::unknown(),
    );

    assert_eq!(observed.system.kind, SystemObservationKind::PipeWire);
    assert_eq!(observed.system.format, AudioFormatObservation::unknown());
    assert_eq!(observed.device.name, None);
    assert_eq!(observed.device.format, AudioFormatObservation::unknown());
}

#[test]
fn mpv_audio_params_populate_only_observed_decoder_values() {
    let decoder = ObservedMpvProperties::from_audio_params(&json!({
        "format": "s24",
        "samplerate": 96000,
        "channel-count": 2
    }));

    assert_eq!(decoder.pcm_format.as_deref(), Some("s24"));
    assert_eq!(decoder.format, format(96000, 24, 2));
}

#[test]
fn pw_dump_reports_graph_format_and_the_routed_output_device() {
    let fixture = json!([
        {
            "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "settings" },
            "metadata": [
                { "subject": 0, "key": "clock.rate", "value": 48000 },
                { "subject": 0, "key": "clock.force-rate", "value": 96000 }
            ]
        },
        {
            "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [{
                "subject": 0,
                "key": "default.audio.sink",
                "value": { "name": "alsa_output.default-speakers" }
            }]
        },
        {
            "id": 80,
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": {
                    "media.class": "Stream/Output/Audio",
                    "application.name": "Earthly Audio",
                    "node.name": "Earthly Audio"
                },
                "params": {
                    "Format": [{
                        "mediaType": "audio",
                        "mediaSubtype": "raw",
                        "format": "S24LE",
                        "rate": 96000,
                        "channels": 2
                    }]
                }
            }
        },
        {
            "id": 81,
            "type": "PipeWire:Interface:Link",
            "info": {
                "output-node-id": 80,
                "input-node-id": 63,
                "state": "active"
            }
        },
        {
            "id": 63,
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": {
                    "media.class": "Audio/Sink",
                    "node.name": "alsa_output.usb-dac",
                    "node.description": "Studio USB DAC"
                },
                "params": {
                    "Format": [{
                        "mediaType": "audio",
                        "mediaSubtype": "raw",
                        "format": "S24LE",
                        "rate": 48000,
                        "channels": 2
                    }]
                }
            }
        }
    ]);

    let observed = parse_pw_dump(fixture.to_string().as_bytes())
        .expect("parse pw-dump")
        .expect("PipeWire observation");

    assert_eq!(observed.graph_format.sample_rate_hz, Some(96000));
    assert_eq!(observed.graph_format.bit_depth, Some(24));
    assert_eq!(observed.graph_format.channels, Some(2));
    assert_eq!(observed.device_name.as_deref(), Some("Studio USB DAC"));
    assert_eq!(observed.device_format, format(48000, 24, 2));
    assert_eq!(observed.is_device_resampling, Some(true));
}

#[test]
fn unavailable_pw_dump_is_reported_as_no_observation() {
    let observer = CommandPipeWireObserver::with_binary(PathBuf::from(
        "/definitely-unavailable/earthly-audio-pw-dump",
    ));

    assert_eq!(
        observer.observe().expect("observe unavailable PipeWire"),
        None
    );
}

#[test]
fn failed_pw_dump_surfaces_diagnostic_context() {
    let binary = temporary_path("failed-pw-dump");
    fs::write(&binary, "#!/bin/sh\necho controlled failure >&2\nexit 23\n")
        .expect("write failed pw-dump fixture");
    let mut permissions = fs::metadata(&binary)
        .expect("fixture metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&binary, permissions).expect("make fixture executable");

    let error = CommandPipeWireObserver::with_binary(binary.clone())
        .observe()
        .expect_err("surface failed pw-dump");

    assert!(error.contains("status 23"));
    assert!(error.contains("controlled failure"));
    fs::remove_file(binary).expect("remove failed pw-dump fixture");
}

fn format(sample_rate_hz: u32, bit_depth: u16, channels: u16) -> AudioFormatObservation {
    AudioFormatObservation {
        sample_rate_hz: Some(sample_rate_hz),
        bit_depth: Some(bit_depth),
        channels: Some(channels),
    }
}

fn temporary_path(file_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("earthly-audio-{}-{file_name}", std::process::id()))
}
