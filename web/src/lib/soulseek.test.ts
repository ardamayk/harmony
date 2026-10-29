import type { ExternalTrack } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import { buildSoulseekQuery, formatAcquisitionQuality } from "./soulseek";

function externalTrack(
	title: string,
	artistName: string | null,
): ExternalTrack {
	return {
		id: "token",
		title,
		artistName,
		durationMs: null,
		webpageUrl: null,
		extractor: "Youtube",
		hasArtwork: false,
	};
}

describe("buildSoulseekQuery", () => {
	it("joins artist and title", () => {
		expect(buildSoulseekQuery(externalTrack("Discovery", "Daft Punk"))).toBe(
			"Daft Punk Discovery",
		);
	});

	it("strips video decorations from the title", () => {
		for (const title of [
			"One More Time (Official Video)",
			"One More Time (Official Audio)",
			"One More Time [Lyric Video]",
			"One More Time (Official Music Video) [HD]",
		]) {
			expect(buildSoulseekQuery(externalTrack(title, "Daft Punk"))).toBe(
				"Daft Punk One More Time",
			);
		}
	});

	it("keeps an artist already in the title once, without the dash", () => {
		expect(
			buildSoulseekQuery(
				externalTrack(
					"Daft Punk - One More Time (Official Video)",
					"Daft Punk",
				),
			),
		).toBe("Daft Punk One More Time");
	});

	it("drops channel suffixes from the artist", () => {
		expect(
			buildSoulseekQuery(
				externalTrack("Around the World", "Daft Punk - Topic"),
			),
		).toBe("Daft Punk Around the World");
		expect(buildSoulseekQuery(externalTrack("Hello", "AdeleVEVO"))).toBe(
			"Adele Hello",
		);
	});

	it("uses the title alone without an artist", () => {
		expect(buildSoulseekQuery(externalTrack("Untitled", null))).toBe(
			"Untitled",
		);
	});
});

describe("formatAcquisitionQuality", () => {
	it("names lossless bit depth and sample rate", () => {
		expect(
			formatAcquisitionQuality({
				format: "flac",
				lossless: true,
				bitDepth: 24,
				sampleRateHz: 48_000,
			}),
		).toBe("FLAC 24-bit / 48 kHz");
		expect(
			formatAcquisitionQuality({
				format: "flac",
				lossless: true,
				bitDepth: 16,
				sampleRateHz: 44_100,
			}),
		).toBe("FLAC 16-bit / 44.1 kHz");
	});

	it("names lossy bitrate and VBR apart", () => {
		expect(
			formatAcquisitionQuality({
				format: "mp3",
				lossless: false,
				bitrateKbps: 320,
				variableBitrate: false,
			}),
		).toBe("MP3 320 kbps");
		expect(
			formatAcquisitionQuality({
				format: "mp3",
				lossless: false,
				bitrateKbps: 245,
				variableBitrate: true,
			}),
		).toBe("MP3 VBR 245 kbps");
	});

	it("says unknown instead of guessing missing attributes", () => {
		expect(formatAcquisitionQuality({ format: "flac", lossless: true })).toBe(
			"FLAC, quality unknown",
		);
		expect(
			formatAcquisitionQuality({
				format: "flac",
				lossless: true,
				sampleRateHz: 96_000,
			}),
		).toBe("FLAC unknown bit depth / 96 kHz");
		expect(formatAcquisitionQuality({ format: "mp3", lossless: false })).toBe(
			"MP3, bitrate unknown",
		);
	});
});
