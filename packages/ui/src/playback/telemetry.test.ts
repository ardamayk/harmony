import type { RadioSearchResult, RadioStation, Track } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import {
	createBrowserPlaybackTelemetry,
	createFallbackPlaybackTelemetry,
	derivePlaybackTelemetryStatus,
	deriveReplayGainAvailability,
	describePlaybackTelemetry,
	mergeProcessingState,
	type PlaybackTelemetry,
} from "./telemetry";

const MATCHED_TELEMETRY: PlaybackTelemetry = {
	source: {
		codec: "FLAC",
		bitrateKbps: 1411,
		format: { sampleRateHz: 96000, bitDepth: 24, channels: 2 },
	},
	decoder: {
		pcmFormat: "s24",
		format: { sampleRateHz: 96000, bitDepth: 24, channels: 2 },
	},
	system: {
		kind: "pipewire",
		format: { sampleRateHz: 96000, bitDepth: 24, channels: 2 },
		isResampling: false,
	},
	device: {
		name: "USB DAC",
		format: { sampleRateHz: 96000, bitDepth: 24, channels: 2 },
		isResampling: false,
	},
	processing: {
		profile: "direct",
		softwareVolume: 1,
		replayGainMode: "off",
		effectiveReplayGainMode: "off",
		isEqualizerEnabled: false,
	},
};

describe("derivePlaybackTelemetryStatus", () => {
	it("reports Format matched only when observed formats agree without processing", () => {
		expect(derivePlaybackTelemetryStatus(MATCHED_TELEMETRY)).toBe(
			"format-matched",
		);
	});

	it("matches the real Track contract without inventing unavailable Source channels", () => {
		expect(
			derivePlaybackTelemetryStatus({
				...MATCHED_TELEMETRY,
				source: {
					...MATCHED_TELEMETRY.source,
					format: {
						...MATCHED_TELEMETRY.source.format,
						channels: null,
					},
				},
			}),
		).toBe("format-matched");
	});

	it("reports Processed from explicit active processing evidence", () => {
		expect(
			derivePlaybackTelemetryStatus({
				...MATCHED_TELEMETRY,
				processing: {
					...MATCHED_TELEMETRY.processing,
					profile: "processed",
					replayGainMode: "album",
					effectiveReplayGainMode: "album",
				},
			}),
		).toBe("processed");
	});

	it("reports Resampled from observed PipeWire negotiation", () => {
		expect(
			derivePlaybackTelemetryStatus({
				...MATCHED_TELEMETRY,
				system: {
					kind: "pipewire",
					format: { sampleRateHz: 48000, bitDepth: 24, channels: 2 },
					isResampling: true,
				},
			}),
		).toBe("resampled");
	});

	it("reports Unknown when the system path is only partially observed", () => {
		expect(
			derivePlaybackTelemetryStatus({
				...MATCHED_TELEMETRY,
				system: {
					kind: "pipewire",
					format: {
						sampleRateHz: null,
						bitDepth: null,
						channels: null,
					},
					isResampling: null,
				},
				device: {
					name: null,
					format: {
						sampleRateHz: null,
						bitDepth: null,
						channels: null,
					},
					isResampling: null,
				},
			}),
		).toBe("unknown");
	});

	it("does not report an end-to-end match without observed Device format", () => {
		expect(
			derivePlaybackTelemetryStatus({
				...MATCHED_TELEMETRY,
				device: {
					name: null,
					format: unknownFormat(),
					isResampling: null,
				},
			}),
		).toBe("unknown");
	});

	it("reports Unknown when no layer contains usable evidence", () => {
		expect(
			derivePlaybackTelemetryStatus({
				source: {
					codec: null,
					bitrateKbps: null,
					format: unknownFormat(),
				},
				decoder: { pcmFormat: null, format: unknownFormat() },
				system: {
					kind: "unknown",
					format: unknownFormat(),
					isResampling: null,
				},
				device: {
					name: null,
					format: unknownFormat(),
					isResampling: null,
				},
				processing: {
					profile: "unknown",
					softwareVolume: null,
					replayGainMode: "unknown",
					effectiveReplayGainMode: "unknown",
					isEqualizerEnabled: null,
				},
			}),
		).toBe("unknown");
	});
});

describe("describePlaybackTelemetry", () => {
	it("formats each independently observed layer for presentation", () => {
		expect(describePlaybackTelemetry(MATCHED_TELEMETRY)).toEqual({
			source: "FLAC · 1411 kbps · 96 kHz · 24-bit · 2 ch",
			decoder: "S24 · 96 kHz · 24-bit · 2 ch",
			system: "PipeWire · 96 kHz · 24-bit · 2 ch",
			device: "USB DAC · 96 kHz · 24-bit · 2 ch",
			processing: "Direct · Volume 100% · ReplayGain Off · EQ Off",
		});
	});

	it("distinguishes requested Album mode from an effective Track fallback", () => {
		const telemetry = {
			...MATCHED_TELEMETRY,
			processing: {
				...MATCHED_TELEMETRY.processing,
				profile: "processed" as const,
				replayGainMode: "album" as const,
				effectiveReplayGainMode: "track-fallback" as const,
			},
		};

		expect(describePlaybackTelemetry(telemetry).processing).toContain(
			"Effective Track fallback",
		);
	});
});

describe("deriveReplayGainAvailability", () => {
	it("identifies Track fallback when Album metadata is unavailable", () => {
		expect(
			deriveReplayGainAvailability(
				{
					trackGainDb: -7.25,
					trackPeak: 0.9,
					albumGainDb: null,
					albumPeak: null,
				},
				"track-fallback",
			),
		).toEqual({
			isTrackAvailable: true,
			isAlbumAvailable: false,
			isUsingTrackFallback: true,
		});
	});
});

describe("createBrowserPlaybackTelemetry", () => {
	it("uses source metadata without inventing decoder or DAC observations", () => {
		const track = {
			format: "flac",
			bitrateKbps: 1411,
			sampleRateHz: 96000,
			bitDepth: 24,
		} as Track;

		const telemetry = createBrowserPlaybackTelemetry(track, 1);

		expect(telemetry.source).toMatchObject({
			codec: "FLAC",
			bitrateKbps: 1411,
			format: { sampleRateHz: 96000, bitDepth: 24 },
		});
		expect(telemetry.decoder.format).toEqual(unknownFormat());
		expect(telemetry.system.kind).toBe("browser-managed");
		expect(telemetry.device.name).toBeNull();
		expect(derivePlaybackTelemetryStatus(telemetry)).toBe("unknown");
	});
});

describe("createFallbackPlaybackTelemetry", () => {
	it("uses saved Radio Station metadata as Source evidence", () => {
		const station = {
			id: "radio-1",
			name: "Radio",
			streamUrl: "https://example.com/radio",
			tags: [],
			source: "manual",
			isFavorite: false,
			position: 0,
			codec: "aac+",
			bitrate: 192,
		} as RadioStation;

		const telemetry = createFallbackPlaybackTelemetry(
			{
				type: "radio-station",
				station,
				playbackUrl: "/radio/radio-1",
				sourceUrl: station.streamUrl,
			},
			1,
		);

		expect(telemetry.source).toEqual({
			codec: "AAC+",
			bitrateKbps: 192,
			format: unknownFormat(),
		});
		expect(telemetry.system.kind).toBe("browser-managed");
	});

	it("uses Catalog Preview metadata as Source evidence", () => {
		const result = {
			stationUuid: "catalog-1",
			name: "Catalog Radio",
			streamUrl: "https://example.com/catalog",
			tags: [],
			codec: "ogg",
			bitrate: 256,
		} as RadioSearchResult;

		const telemetry = createFallbackPlaybackTelemetry(
			{
				type: "catalog-preview",
				result,
				playbackUrl: "/radio/catalog-1",
				sourceUrl: result.streamUrl,
			},
			1,
		);

		expect(telemetry.source).toMatchObject({
			codec: "OGG",
			bitrateKbps: 256,
		});
	});
});

describe("mergeProcessingState", () => {
	it("projects the effective processing state without changing the observed path", () => {
		const telemetry = mergeProcessingState(MATCHED_TELEMETRY, {
			profile: "processed",
			softwareVolume: 0.4,
			replayGainMode: "album",
			effectiveReplayGainMode: "track-fallback",
			replayGainPreference: "album",
			equalizer: {
				isEnabled: true,
				preset: "custom",
				gainsDb: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
			},
			effectiveAudioFilters: ["equalizer=f=31.25:t=q:w=1:g=3"],
			transitionNotice: null,
		});

		expect(telemetry.processing).toEqual({
			profile: "processed",
			softwareVolume: 0.4,
			replayGainMode: "album",
			effectiveReplayGainMode: "track-fallback",
			isEqualizerEnabled: true,
		});
		expect(telemetry.system).toEqual(MATCHED_TELEMETRY.system);
	});
});

function unknownFormat() {
	return { sampleRateHz: null, bitDepth: null, channels: null };
}
