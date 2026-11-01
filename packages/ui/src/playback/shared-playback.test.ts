import { describe, expect, it } from "vite-plus/test";
import type { PlaybackState } from "@repo/api-client";
import {
	expectedPlayheadSeconds,
	isSnapshotLive,
	SHARED_PLAYBACK_HEARTBEAT_MS,
	SHARED_PLAYBACK_STALE_REPORT_MS,
	snapshotClockOffsetMs,
} from "./shared-playback";

function state(overrides: Partial<PlaybackState> = {}): PlaybackState {
	return {
		revision: 1,
		queueItemId: "item-1",
		trackId: "track-1",
		playing: true,
		positionMs: 10_000,
		positionAtMs: 100_000,
		activeDeviceId: "device-1",
		activeDeviceName: "Harmony on desktop",
		remoteCommandRevision: 0,
		skipRequest: null,
		serverTimeMs: 100_050,
		...overrides,
	};
}

describe("shared playback helpers", () => {
	it("hearts beats every 5 seconds and stale-reports after 20", () => {
		expect(SHARED_PLAYBACK_HEARTBEAT_MS).toBe(5000);
		expect(SHARED_PLAYBACK_STALE_REPORT_MS).toBe(20_000);
	});

	it("computes the clock offset from the snapshot's server time", () => {
		// The snapshot's server time is 100_050 when the local clock is 100_000.
		expect(snapshotClockOffsetMs(state(), 100_000)).toBe(50);
	});

	it("advances the playhead from the stamped position while playing", () => {
		const snapshot = state();
		// Local time 100_150 with offset 50 is server time 100_200:
		// 200 ms after the stamp, so the playhead is at 10.2 s.
		expect(expectedPlayheadSeconds(snapshot, 50, 100_150)).toBe(10.2);
	});

	it("keeps a paused playhead where it was reported", () => {
		const snapshot = state({ playing: false });
		expect(expectedPlayheadSeconds(snapshot, 50, 190_000)).toBe(10);
	});

	it("stands the playhead still once reports stop arriving", () => {
		expect(expectedPlayheadSeconds(state(), 50, 190_000)).toBe(10);
	});

	it("never moves a playhead backwards from a stamp in the future", () => {
		const snapshot = state({ positionAtMs: 200_000 });
		expect(expectedPlayheadSeconds(snapshot, 50, 100_000)).toBe(10);
	});

	it("reads a playing state as live while reports keep arriving", () => {
		const snapshot = state();
		// Server time 100_200 is 200 ms after the stamp: well inside the window.
		expect(isSnapshotLive(snapshot, 50, 100_150)).toBe(true);
	});

	it("reads a playing state as paused once reports stop arriving", () => {
		const snapshot = state();
		// Server time 121_000 is 21 s after the stamp: past the 20 s window.
		expect(isSnapshotLive(snapshot, 50, 120_950)).toBe(false);
	});

	it("reads a paused state as not live without a liveness check", () => {
		expect(isSnapshotLive(state({ playing: false }), 50, 100_000)).toBe(false);
	});
});
