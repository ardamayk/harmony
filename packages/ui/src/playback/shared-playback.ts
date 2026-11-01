import type { PlaybackState } from "@repo/api-client";

/** The Active Device reports this often while playing (ADR 0040). */
export const SHARED_PLAYBACK_HEARTBEAT_MS = 5000;
/** A report older than this reads the state as paused at its playhead. */
export const SHARED_PLAYBACK_STALE_REPORT_MS = 20_000;
/** A playhead further than this from the expected position jumps. */
export const SHARED_PLAYBACK_JUMP_SECONDS = 1.5;

/**
 * Server clock minus local clock at receipt. The snapshot carries the
 * server's time once; one offset advances its playhead without clock
 * sampling (ADR 0040).
 */
export function snapshotClockOffsetMs(
	state: PlaybackState,
	receivedAtMs: number,
): number {
	return state.serverTimeMs - receivedAtMs;
}

/**
 * Where the playhead is at `nowMs`: the reported position, advanced by the
 * time since the server stamped it while the state is live. Paused or
 * stale, it stands still.
 */
export function expectedPlayheadSeconds(
	state: PlaybackState,
	clockOffsetMs: number,
	nowMs: number,
): number {
	if (!isSnapshotLive(state, clockOffsetMs, nowMs)) {
		return state.positionMs / 1000;
	}
	const serverNowMs = nowMs + clockOffsetMs;
	return (
		(state.positionMs + Math.max(0, serverNowMs - state.positionAtMs)) / 1000
	);
}

/**
 * Whether `playing` can still be believed: the Active Device reports every
 * heartbeat, so a state whose report is older than the liveness window
 * reads as paused at its playhead.
 */
export function isSnapshotLive(
	state: PlaybackState,
	clockOffsetMs: number,
	nowMs: number,
): boolean {
	if (!state.playing) return false;
	const serverNowMs = nowMs + clockOffsetMs;
	return serverNowMs - state.positionAtMs <= SHARED_PLAYBACK_STALE_REPORT_MS;
}
