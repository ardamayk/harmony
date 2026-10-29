import type { ExternalTrack } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import {
	buildSoulseekQuery,
	formatAcquisitionQuality,
	formatQualityChoice,
	parseQualityChoice,
	QUALITY_GROUPS,
	qualityChoiceOf,
	qualityMatches,
} from "./soulseek";

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

	it("searches artist and Album for the Album scope when the site reports one", () => {
		const track = {
			...externalTrack("One More Time (Official Video)", "Daft Punk - Topic"),
			album: " Discovery ",
		};
		expect(buildSoulseekQuery(track, "album")).toBe("Daft Punk Discovery");
		expect(buildSoulseekQuery({ ...track, artistName: null }, "album")).toBe(
			"Discovery",
		);
	});

	it("keeps artist and title for the Track scope and without an Album", () => {
		const title = "One More Time (Official Video)";
		expect(
			buildSoulseekQuery(
				{ ...externalTrack(title, "Daft Punk"), album: "Discovery" },
				"track",
			),
		).toBe("Daft Punk One More Time");
		for (const album of [undefined, null, ""]) {
			expect(
				buildSoulseekQuery(
					{ ...externalTrack(title, "Daft Punk"), album },
					"album",
				),
			).toBe("Daft Punk One More Time");
		}
	});
});

describe("quality choices", () => {
	const choice = (key: string) => {
		const parsed = parseQualityChoice(key);
		if (!parsed) throw new Error(`no choice ${key}`);
		return parsed;
	};

	it("keys and names each listed choice", () => {
		expect(choice("flac-24-88200")).toMatchObject({
			format: "flac",
			lossless: true,
			bitDepth: 24,
			sampleRateHz: 88_200,
		});
		expect(formatQualityChoice(choice("flac-24-88200"))).toBe("FLAC 24/88.2");
		expect(formatQualityChoice(choice("wav-16-48000"))).toBe("WAV 16/48");
		expect(formatQualityChoice(choice("m4a-256"))).toBe("AAC (M4A) 256 kbps");
		expect(formatQualityChoice(choice("ogg-160"))).toBe("Ogg Vorbis 160 kbps");
		expect(formatQualityChoice(choice("opus-96"))).toBe("Opus 96 kbps");
		expect(parseQualityChoice("mp3-224")).toBeNull();
		expect(parseQualityChoice("garbage")).toBeNull();
		expect(parseQualityChoice(null)).toBeNull();
		expect(QUALITY_GROUPS.map((group) => group.format)).toEqual([
			"mp3",
			"m4a",
			"opus",
			"ogg",
			"flac",
			"wav",
		]);
	});

	it("matches lossless by format, bit depth and sample rate", () => {
		const flac = choice("flac-24-48000");
		const quality = {
			format: "flac",
			lossless: true,
			bitDepth: 24,
			sampleRateHz: 48_000,
		};
		expect(qualityMatches(quality, flac)).toBe(true);
		expect(qualityMatches({ ...quality, sampleRateHz: 44_100 }, flac)).toBe(
			false,
		);
		expect(qualityMatches({ ...quality, format: "wav" }, flac)).toBe(false);
		expect(qualityMatches({ format: "flac", lossless: true }, flac)).toBe(
			false,
		);
	});

	it("matches lossy by a constant or unreported-mode bitrate, never a variable one", () => {
		const mp3 = choice("mp3-320");
		const quality = { format: "mp3", lossless: false, bitrateKbps: 320 };
		expect(qualityMatches({ ...quality, variableBitrate: false }, mp3)).toBe(
			true,
		);
		expect(qualityMatches(quality, mp3)).toBe(true);
		expect(qualityMatches({ ...quality, variableBitrate: true }, mp3)).toBe(
			false,
		);
		expect(qualityMatches({ ...quality, bitrateKbps: 256 }, mp3)).toBe(false);
	});

	it("maps a found quality to its listed choice when there is one", () => {
		expect(
			qualityChoiceOf({ format: "m4a", lossless: false, bitrateKbps: 256 })
				?.key,
		).toBe("m4a-256");
		expect(
			qualityChoiceOf({ format: "mp3", lossless: false, bitrateKbps: 224 }),
		).toBeNull();
		expect(qualityChoiceOf({ format: "flac", lossless: true })).toBeNull();
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
		).toBe("MP3 VBR ~245 kbps");
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
		expect(
			formatAcquisitionQuality({
				format: "mp3",
				lossless: false,
				variableBitrate: true,
			}),
		).toBe("MP3 VBR, bitrate unknown");
	});
});
