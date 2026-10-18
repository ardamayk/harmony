import { describe, expect, it } from "vite-plus/test";
import type { ProcessingState } from "../playback/processing";
import type { PlaybackTelemetry } from "../playback/telemetry";
import { buildQualityDetailRows } from "./QualityDetailsCard";

const telemetry: PlaybackTelemetry = {
	source: {
		codec: "FLAC",
		bitrateKbps: 1411,
		format: { sampleRateHz: 44100, bitDepth: 16, channels: 2 },
	},
	decoder: {
		pcmFormat: "s16",
		format: { sampleRateHz: 44100, bitDepth: 16, channels: 2 },
	},
	system: {
		kind: "pipewire",
		format: { sampleRateHz: 48000, bitDepth: 32, channels: 2 },
		isResampling: true,
	},
	device: {
		name: "USB DAC",
		format: { sampleRateHz: 48000, bitDepth: 32, channels: 2 },
		isResampling: null,
	},
	processing: {
		profile: "direct",
		softwareVolume: 1,
		replayGainMode: "off",
		effectiveReplayGainMode: "off",
		isEqualizerEnabled: false,
	},
};

const processing: ProcessingState = {
	profile: "processed",
	softwareVolume: 0.8,
	replayGainMode: "album",
	effectiveReplayGainMode: "track-fallback",
	replayGainPreference: null,
	equalizer: {
		isEnabled: true,
		preset: "flat",
		gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
	},
	effectiveAudioFilters: [],
	transitionNotice: null,
};

describe("buildQualityDetailRows", () => {
	it("returns nothing without telemetry, processing or output mode", () => {
		expect(
			buildQualityDetailRows({
				telemetry: null,
				processing: null,
				outputMode: null,
			}),
		).toEqual([]);
	});

	it("lists source, output, resampling, output mode and ReplayGain", () => {
		const rows = buildQualityDetailRows({
			telemetry,
			processing,
			outputMode: "direct-alsa",
		});
		const byLabel = Object.fromEntries(
			rows.map((row) => [row.label, row.value]),
		);
		expect(byLabel.Source).toContain("FLAC");
		expect(byLabel.Source).toContain("1411 kbps");
		expect(byLabel.Resampling).toBe("Yes");
		expect(byLabel["Output mode"]).toBe("Exclusive");
		expect(byLabel.ReplayGain).toBe("Track (album gain missing)");
		expect(byLabel.Equalizer).toBe("On");
	});
});
