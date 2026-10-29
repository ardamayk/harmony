import { describe, expect, it } from "vite-plus/test";
import {
	API_V1_CAPABILITY,
	hasServerCapability,
	MANAGED_IMPORT_BATCHES_CAPABILITY,
	MANAGED_IMPORT_CAPABILITY,
	MANAGED_TRACK_DELETION_CAPABILITY,
	MANAGED_TRACK_REPLACEMENT_CAPABILITY,
	missingServerCapabilities,
} from "./capabilities";

const currentServer = {
	status: "ok" as const,
	version: "0.1.0",
	capabilities: [
		API_V1_CAPABILITY,
		"playback.queue-events.v1",
		MANAGED_IMPORT_CAPABILITY,
		MANAGED_IMPORT_BATCHES_CAPABILITY,
		MANAGED_TRACK_DELETION_CAPABILITY,
		MANAGED_TRACK_REPLACEMENT_CAPABILITY,
	],
	dependencies: [
		{ name: "ffmpeg", required: true, available: true, version: "7.1" },
	],
};

describe("Server Capability gating", () => {
	it("accepts either a health response or a bare capability list", () => {
		expect(hasServerCapability(currentServer, MANAGED_IMPORT_CAPABILITY)).toBe(
			true,
		);
		expect(
			hasServerCapability(
				currentServer.capabilities,
				MANAGED_TRACK_REPLACEMENT_CAPABILITY,
			),
		).toBe(true);
	});

	it("gates on the exact capability name only", () => {
		const olderServer = [
			API_V1_CAPABILITY,
			"managed-import.v2",
			"managed-import",
		];
		expect(hasServerCapability(olderServer, MANAGED_IMPORT_CAPABILITY)).toBe(
			false,
		);
		expect(hasServerCapability(olderServer, "managed-import.v2")).toBe(true);
	});

	it("ignores unknown optional capabilities from a newer Music Server", () => {
		const newerServer = [
			...currentServer.capabilities,
			"future.optional-feature.v9",
			"playback.party-mode.v1",
		];
		expect(hasServerCapability(newerServer, MANAGED_IMPORT_CAPABILITY)).toBe(
			true,
		);
		expect(hasServerCapability(newerServer, "unknown.capability")).toBe(false);
		expect(
			missingServerCapabilities(newerServer, [
				API_V1_CAPABILITY,
				MANAGED_IMPORT_BATCHES_CAPABILITY,
				MANAGED_TRACK_DELETION_CAPABILITY,
			]),
		).toEqual([]);
	});

	it("names every required capability an older Music Server lacks", () => {
		const olderServer = [API_V1_CAPABILITY, MANAGED_IMPORT_CAPABILITY];
		expect(
			missingServerCapabilities(olderServer, [
				API_V1_CAPABILITY,
				MANAGED_IMPORT_BATCHES_CAPABILITY,
				MANAGED_TRACK_REPLACEMENT_CAPABILITY,
			]),
		).toEqual([
			MANAGED_IMPORT_BATCHES_CAPABILITY,
			MANAGED_TRACK_REPLACEMENT_CAPABILITY,
		]);
	});

	it("treats a missing health response as advertising nothing", () => {
		expect(hasServerCapability(undefined, API_V1_CAPABILITY)).toBe(false);
		expect(hasServerCapability(null, API_V1_CAPABILITY)).toBe(false);
		expect(missingServerCapabilities(undefined, [API_V1_CAPABILITY])).toEqual([
			API_V1_CAPABILITY,
		]);
	});
});

describe("track waveform capability", () => {
	it("is gated on the exact advertised name", async () => {
		const { TRACK_WAVEFORM_CAPABILITY, hasServerCapability } =
			await import("./capabilities");
		expect(TRACK_WAVEFORM_CAPABILITY).toBe("track-waveform.v1");
		expect(
			hasServerCapability(
				["api.v1", "track-waveform.v1"],
				TRACK_WAVEFORM_CAPABILITY,
			),
		).toBe(true);
		expect(hasServerCapability(["api.v1"], TRACK_WAVEFORM_CAPABILITY)).toBe(
			false,
		);
	});
});

describe("external source capability", () => {
	it("is gated on the exact advertised name", async () => {
		const { EXTERNAL_SOURCE_CAPABILITY, hasServerCapability } =
			await import("./capabilities");
		expect(EXTERNAL_SOURCE_CAPABILITY).toBe("external-source.v1");
		expect(
			hasServerCapability(
				["api.v1", "external-source.v1"],
				EXTERNAL_SOURCE_CAPABILITY,
			),
		).toBe(true);
		expect(
			hasServerCapability(
				["api.v1", "external-source.v2"],
				EXTERNAL_SOURCE_CAPABILITY,
			),
		).toBe(false);
	});
});
