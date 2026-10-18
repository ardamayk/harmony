import type { Track } from "@repo/api-client";
import type { PlaybackSource } from "./PlaybackEngine";
import type {
	EffectiveReplayGainMode,
	ProcessingState,
	ReplayGainMode,
} from "./processing";

export type AudioFormatObservation = {
	sampleRateHz: number | null;
	bitDepth: number | null;
	channels: number | null;
};

export type PlaybackTelemetry = {
	source: {
		codec: string | null;
		bitrateKbps: number | null;
		format: AudioFormatObservation;
	};
	decoder: {
		pcmFormat: string | null;
		format: AudioFormatObservation;
	};
	system: {
		kind: "pipewire" | "browser-managed" | "bypassed" | "unknown";
		format: AudioFormatObservation;
		isResampling: boolean | null;
	};
	device: {
		name: string | null;
		format: AudioFormatObservation;
		isResampling: boolean | null;
	};
	processing: {
		profile: "direct" | "processed" | "unknown";
		softwareVolume: number | null;
		replayGainMode: ReplayGainMode | "unknown";
		effectiveReplayGainMode: EffectiveReplayGainMode;
		isEqualizerEnabled: boolean | null;
	};
};

export type PlaybackTelemetryStatus =
	| "format-matched"
	| "processed"
	| "resampled"
	| "unknown";

export type PlaybackTelemetryDescriptions = {
	source: string;
	decoder: string;
	system: string;
	device: string;
	processing: string;
};

export type ReplayGainAvailability = {
	isTrackAvailable: boolean;
	isAlbumAvailable: boolean;
	isUsingTrackFallback: boolean;
};

export function derivePlaybackTelemetryStatus(
	telemetry: PlaybackTelemetry,
): PlaybackTelemetryStatus {
	if (hasActiveProcessing(telemetry.processing)) return "processed";
	if (hasResamplingEvidence(telemetry)) return "resampled";
	if (hasFormatMatchEvidence(telemetry)) return "format-matched";
	return "unknown";
}

export function createBrowserPlaybackTelemetry(
	track: Track | null,
	softwareVolume: number,
): PlaybackTelemetry {
	return {
		source: {
			codec: track?.format?.toUpperCase() ?? null,
			bitrateKbps: track?.bitrateKbps ?? null,
			format: {
				sampleRateHz: track?.sampleRateHz ?? null,
				bitDepth: track?.bitDepth ?? null,
				channels: null,
			},
		},
		decoder: { pcmFormat: null, format: unknownFormat() },
		system: {
			kind: "browser-managed",
			format: unknownFormat(),
			isResampling: null,
		},
		device: { name: null, format: unknownFormat(), isResampling: null },
		processing: {
			profile: softwareVolume === 1 ? "unknown" : "processed",
			softwareVolume,
			replayGainMode: "unknown",
			effectiveReplayGainMode: "unknown",
			isEqualizerEnabled: null,
		},
	};
}

export function createFallbackPlaybackTelemetry(
	source: PlaybackSource | null,
	softwareVolume: number,
): PlaybackTelemetry {
	const track = source?.type === "track" ? source.track : null;
	const telemetry = createBrowserPlaybackTelemetry(track, softwareVolume);
	if (!source || source.type === "track") return telemetry;
	const radio =
		source.type === "radio-station" ? source.station : source.result;
	return {
		...telemetry,
		source: {
			...telemetry.source,
			codec: radio.codec?.toUpperCase() ?? null,
			bitrateKbps: radio.bitrate ?? null,
		},
	};
}

export function mergeProcessingState(
	telemetry: PlaybackTelemetry,
	state: ProcessingState | null,
): PlaybackTelemetry {
	if (!state) return telemetry;
	return {
		...telemetry,
		processing: {
			profile: state.profile,
			softwareVolume: state.softwareVolume,
			replayGainMode: state.replayGainMode,
			effectiveReplayGainMode: state.effectiveReplayGainMode,
			isEqualizerEnabled: state.equalizer.isEnabled,
		},
	};
}

export function formatTelemetryStatus(status: PlaybackTelemetryStatus) {
	switch (status) {
		case "format-matched":
			return "Format matched";
		case "processed":
			return "Processed";
		case "resampled":
			return "Resampled";
		case "unknown":
			return "Unknown";
	}
}

export function describePlaybackTelemetry(
	telemetry: PlaybackTelemetry,
): PlaybackTelemetryDescriptions {
	return {
		source: describeSource(telemetry),
		decoder: describeDecoder(telemetry),
		system: describeSystem(telemetry),
		device: describeDevice(telemetry),
		processing: describeProcessing(telemetry),
	};
}

export function deriveReplayGainAvailability(
	metadata: Track["replayGain"] | undefined,
	effectiveMode: EffectiveReplayGainMode,
): ReplayGainAvailability {
	const isTrackAvailable = metadata?.trackGainDb != null;
	const isAlbumAvailable = metadata?.albumGainDb != null;
	return {
		isTrackAvailable,
		isAlbumAvailable,
		isUsingTrackFallback: effectiveMode === "track-fallback",
	};
}

function describeSource(telemetry: PlaybackTelemetry) {
	return joinKnown([
		telemetry.source.codec,
		telemetry.source.bitrateKbps
			? `${telemetry.source.bitrateKbps} kbps`
			: null,
		describeAudioFormat(telemetry.source.format),
	]);
}

function describeDecoder(telemetry: PlaybackTelemetry) {
	return joinKnown([
		telemetry.decoder.pcmFormat?.toUpperCase() ?? null,
		describeAudioFormat(telemetry.decoder.format),
	]);
}

function describeSystem(telemetry: PlaybackTelemetry) {
	const labels = {
		pipewire: "PipeWire",
		"browser-managed": "Browser managed",
		bypassed: "Bypassed",
		unknown: "Unknown",
	};
	return joinKnown([
		labels[telemetry.system.kind],
		describeAudioFormat(telemetry.system.format),
	]);
}

function describeDevice(telemetry: PlaybackTelemetry) {
	return joinKnown([
		telemetry.device.name,
		describeAudioFormat(telemetry.device.format),
	]);
}

function describeProcessing(telemetry: PlaybackTelemetry) {
	const { processing } = telemetry;
	return joinKnown([
		capitalize(processing.profile),
		processing.softwareVolume === null
			? null
			: `Volume ${Math.round(processing.softwareVolume * 100)}%`,
		processing.replayGainMode === "unknown"
			? null
			: `ReplayGain ${capitalize(processing.replayGainMode)}`,
		processing.effectiveReplayGainMode === "unknown" ||
		processing.effectiveReplayGainMode === processing.replayGainMode
			? null
			: `Effective ${formatEffectiveReplayGain(processing.effectiveReplayGainMode)}`,
		processing.isEqualizerEnabled === null
			? null
			: `EQ ${processing.isEqualizerEnabled ? "On" : "Off"}`,
	]);
}

function describeAudioFormat(format: AudioFormatObservation) {
	if (Object.values(format).every((value) => value === null)) return null;
	return joinKnown([
		format.sampleRateHz ? `${format.sampleRateHz / 1000} kHz` : null,
		format.bitDepth ? `${format.bitDepth}-bit` : null,
		format.channels ? `${format.channels} ch` : null,
	]);
}

function joinKnown(values: Array<string | null>) {
	const known = values.filter((value): value is string => Boolean(value));
	return known.length > 0 ? known.join(" · ") : "Unknown";
}

function capitalize(value: string) {
	return value.charAt(0).toUpperCase() + value.slice(1);
}

function formatEffectiveReplayGain(mode: EffectiveReplayGainMode) {
	return mode === "track-fallback" ? "Track fallback" : capitalize(mode);
}

function hasActiveProcessing(processing: PlaybackTelemetry["processing"]) {
	return (
		processing.profile === "processed" ||
		(processing.softwareVolume !== null && processing.softwareVolume !== 1) ||
		(processing.replayGainMode !== "off" &&
			processing.replayGainMode !== "unknown") ||
		processing.isEqualizerEnabled === true
	);
}

function hasResamplingEvidence(telemetry: PlaybackTelemetry) {
	if (
		telemetry.system.isResampling === true ||
		telemetry.device.isResampling === true
	)
		return true;
	const formats = [
		telemetry.source.format,
		telemetry.decoder.format,
		telemetry.system.format,
		telemetry.device.format,
	];
	return formats.some((format, index) => {
		const next = formats[index + 1];
		return Boolean(
			format.sampleRateHz &&
			next?.sampleRateHz &&
			format.sampleRateHz !== next.sampleRateHz,
		);
	});
}

function hasFormatMatchEvidence(telemetry: PlaybackTelemetry) {
	const { source, decoder, system, device } = telemetry;
	if (!hasRequiredSourceFormat(source.format)) return false;
	if (![decoder.format, system.format, device.format].every(isCompleteFormat))
		return false;
	return (
		knownFormatsMatch(source.format, decoder.format) &&
		formatsMatch(decoder.format, system.format) &&
		formatsMatch(system.format, device.format)
	);
}

function hasRequiredSourceFormat(format: AudioFormatObservation) {
	return format.sampleRateHz !== null && format.bitDepth !== null;
}

function knownFormatsMatch(
	left: AudioFormatObservation,
	right: AudioFormatObservation,
) {
	return (Object.keys(left) as Array<keyof AudioFormatObservation>).every(
		(key) =>
			left[key] === null || right[key] === null || left[key] === right[key],
	);
}

function formatsMatch(
	left: AudioFormatObservation,
	right: AudioFormatObservation,
) {
	return (
		left.sampleRateHz === right.sampleRateHz &&
		left.bitDepth === right.bitDepth &&
		left.channels === right.channels
	);
}

function isCompleteFormat(format: AudioFormatObservation) {
	return (
		format.sampleRateHz !== null &&
		format.bitDepth !== null &&
		format.channels !== null
	);
}

function unknownFormat(): AudioFormatObservation {
	return { sampleRateHz: null, bitDepth: null, channels: null };
}
