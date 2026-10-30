import type {
	ListenTogetherCommand,
	ListenTogetherRoom,
} from "@repo/api-client";
import type { PlaybackStatus } from "../playback/PlaybackEngine";

/** Sync checks the Player against the Room State this often. */
export const SYNC_TICK_MS = 250;
/** A Player further than this from the expected playhead seeks. */
export const DRIFT_THRESHOLD_MS = 400;
/** No drift correction this soon after a seek, so a buffering Player settles. */
export const SEEK_COOLDOWN_MS = 2000;
/** Corrections in a row back off to this, for a Player that never settles. */
export const MAX_SEEK_COOLDOWN_MS = 16_000;
/** A playhead that jumps further than this without sync moved it is a local seek. */
export const LOCAL_SEEK_THRESHOLD_MS = 2000;
/** How long a change sync made may take to show in the Player's state. */
export const ECHO_WINDOW_MS = 1500;
/** A correction that has not shown yet is repeated this often. */
export const CORRECTION_RETRY_MS = 1000;
/** A Track load older than this may start again. */
export const LOAD_TIMEOUT_MS = 15_000;
/** Seeking this close to the end of a Track would only end it early. */
export const END_MARGIN_MS = 500;

/** The Player's state, read from the PlaybackEngine. */
export type LocalPlayback = {
	/** The source's Queue item id; null without a Track source. */
	queueItemId: string | null;
	hasSource: boolean;
	status: PlaybackStatus;
	positionMs: number;
	/** Local time at which positionMs was reported. */
	observedAtMs: number;
	durationMs: number;
};

export type RoomSyncInput = {
	room: ListenTogetherRoom;
	/** Server clock minus local clock; null until the first estimate. */
	clockOffsetMs: number | null;
	local: LocalPlayback;
	nowMs: number;
};

export type RoomSyncAction =
	| { type: "load"; queueItemId: string; index: number; trackId: string }
	| { type: "play" }
	| { type: "pause" }
	| { type: "seek"; positionMs: number }
	| { type: "command"; command: ListenTogetherCommand };

/** What sync remembers between two plans. */
export type RoomSyncMemo = {
	/** The Track load in flight; repeated snapshots never restart it. */
	loadingId: string | null;
	loadStartedAtMs: number;
	/** The next plan after a load moves the Player to the expected playhead. */
	needsInitialSeek: boolean;
	/** A Track that failed here, so it is not reloaded on every tick. */
	failedId: string | null;
	/** The Track whose end this Player reported. */
	endReportedId: string | null;
	/** A command from a local change is on its way; the Player waits for it. */
	commandPending: boolean;
	/** The Player has shown the Room's play or pause since it last changed. */
	statusInSync: boolean;
	/** The Player has shown the Room's Track. */
	sourceInSync: boolean;
	lastRoomPlaying: boolean | null;
	echoUntilMs: number;
	cooldownUntilMs: number;
	consecutiveSeeks: number;
	lastCorrectionAtMs: number;
	lastPositionMs: number | null;
	lastPositionId: string | null;
	lastTickAtMs: number;
	lastExpectedMs: number | null;
};

export const INITIAL_ROOM_SYNC_MEMO: RoomSyncMemo = {
	loadingId: null,
	loadStartedAtMs: 0,
	needsInitialSeek: false,
	failedId: null,
	endReportedId: null,
	commandPending: false,
	statusInSync: false,
	sourceInSync: false,
	lastRoomPlaying: null,
	echoUntilMs: 0,
	cooldownUntilMs: 0,
	consecutiveSeeks: 0,
	lastCorrectionAtMs: Number.NEGATIVE_INFINITY,
	lastPositionMs: null,
	lastPositionId: null,
	lastTickAtMs: 0,
	lastExpectedMs: null,
};

/**
 * The Queue item id of a place in the Room Queue. A Track can appear twice in
 * a Room Queue, so the id carries the index as well.
 */
export function roomQueueItemId(index: number, trackId: string): string {
	return `room:${index}:${trackId}`;
}

export function isRoomQueueItemId(id: string): boolean {
	return id.startsWith("room:");
}

/** Clients ignore snapshots older than the newest they have seen. */
export function isStaleSnapshot(
	room: ListenTogetherRoom,
	newestRevision: number | null,
): boolean {
	return newestRevision !== null && room.revision < newestRevision;
}

export function expectedPositionMs(
	room: ListenTogetherRoom,
	serverNowMs: number,
): number {
	if (!room.playing) return room.positionMs;
	return room.positionMs + Math.max(0, serverNowMs - room.positionAtMs);
}

export type ClockSample = {
	sentAtMs: number;
	receivedAtMs: number;
	serverTimeMs: number;
};

/** Offset from the sample with the lowest round trip, at its midpoint. */
export function estimateClockOffset(samples: ClockSample[]): number | null {
	let best: ClockSample | null = null;
	for (const sample of samples) {
		if (
			best === null ||
			sample.receivedAtMs - sample.sentAtMs < best.receivedAtMs - best.sentAtMs
		) {
			best = sample;
		}
	}
	if (best === null) return null;
	return best.serverTimeMs - (best.sentAtMs + best.receivedAtMs) / 2;
}

function isAudible(status: PlaybackStatus): boolean {
	return status === "playing" || status === "reconnecting";
}

/** The Room's current place in its Queue; null before the first play. */
function roomTarget(room: ListenTogetherRoom) {
	const index = room.currentIndex;
	if (index === null || index === undefined) return null;
	const trackId = room.trackIds[index];
	if (trackId === undefined) return null;
	return { index, trackId, queueItemId: roomQueueItemId(index, trackId) };
}

/** The Player finished loading, or the load failed. */
export function settleLoad(
	memo: RoomSyncMemo,
	queueItemId: string,
	succeeded: boolean,
): RoomSyncMemo {
	if (memo.loadingId !== queueItemId) return memo;
	return {
		...memo,
		loadingId: null,
		failedId: succeeded ? memo.failedId : queueItemId,
	};
}

/**
 * The command from a local change was answered. Whether or not the Music
 * Server took it, the Player now follows the Room State again, so a refused
 * command puts the Player back where the Room is.
 */
export function settleCommand(memo: RoomSyncMemo): RoomSyncMemo {
	return {
		...memo,
		commandPending: false,
		statusInSync: false,
		sourceInSync: false,
		lastPositionMs: null,
	};
}

/**
 * One step of sync: compares the Room State, the expected playhead and the
 * Player, and returns what to do. Pure, so every case is a test. Call it
 * regularly, not from the engine's own events: a browser reports "paused"
 * just before "ended", and only a later look tells them apart.
 */
export function planRoomSync(
	memo: RoomSyncMemo,
	input: RoomSyncInput,
): { memo: RoomSyncMemo; actions: RoomSyncAction[] } {
	const { local, nowMs } = input;
	const next: RoomSyncMemo = { ...memo };
	if (input.room.playing !== memo.lastRoomPlaying) {
		next.lastRoomPlaying = input.room.playing;
		next.statusInSync = false;
	}
	const expectedMs =
		input.clockOffsetMs === null
			? null
			: expectedPositionMs(input.room, nowMs + input.clockOffsetMs);
	const actions = plan(memo, next, input, expectedMs);
	next.lastPositionMs = local.positionMs;
	next.lastPositionId = local.queueItemId;
	next.lastTickAtMs = nowMs;
	next.lastExpectedMs = expectedMs;
	return { memo: next, actions };
}

function plan(
	previous: RoomSyncMemo,
	next: RoomSyncMemo,
	input: RoomSyncInput,
	expectedMs: number | null,
): RoomSyncAction[] {
	const { room, local, nowMs } = input;
	if (next.commandPending) return [];

	const target = roomTarget(room);
	if (!target) {
		// Nothing to follow yet: the Room starts silent.
		return isAudible(local.status) ? correctStatus(next, nowMs, false) : [];
	}

	if (next.loadingId) {
		const isCurrentLoad = next.loadingId === target.queueItemId;
		if (isCurrentLoad && nowMs - next.loadStartedAtMs < LOAD_TIMEOUT_MS) {
			return [];
		}
		next.loadingId = null;
	}

	if (local.queueItemId !== target.queueItemId) {
		return planOtherSource(previous, next, input, target);
	}
	next.sourceInSync = true;

	if (local.status === "error") {
		next.failedId = target.queueItemId;
		return [];
	}
	if (next.failedId === target.queueItemId) next.failedId = null;

	if (local.status === "ended") {
		return planEnded(next, input, target, expectedMs);
	}
	next.endReportedId = null;

	if (isAudible(local.status) !== room.playing) {
		if (next.statusInSync && nowMs >= next.echoUntilMs) {
			next.commandPending = true;
			return [
				{
					type: "command",
					command: { type: room.playing ? "pause" : "resume" },
				},
			];
		}
		return correctStatus(next, nowMs, room.playing);
	}
	const wasStatusInSync = next.statusInSync;
	next.statusInSync = true;

	return planPosition(previous, next, input, expectedMs, wasStatusInSync);
}

/** The Player is not on the Room's Track: a local change, or sync's to make. */
function planOtherSource(
	previous: RoomSyncMemo,
	next: RoomSyncMemo,
	input: RoomSyncInput,
	target: NonNullable<ReturnType<typeof roomTarget>>,
): RoomSyncAction[] {
	const { room, local, nowMs } = input;
	const wasOnTarget =
		next.sourceInSync &&
		previous.lastPositionId === target.queueItemId &&
		nowMs >= next.echoUntilMs;
	if (wasOnTarget) {
		const direction = adjacentDirection(room, target.index, local.queueItemId);
		if (direction) {
			next.commandPending = true;
			return [
				{
					type: "command",
					command: { type: "skip", direction, fromTrackId: target.trackId },
				},
			];
		}
		if (!local.hasSource && room.playing) {
			// Stopped from outside the app, which a Room has no command for.
			next.commandPending = true;
			return [{ type: "command", command: { type: "pause" } }];
		}
	}
	if (next.failedId === target.queueItemId) return [];
	next.loadingId = target.queueItemId;
	next.loadStartedAtMs = nowMs;
	next.needsInitialSeek = true;
	next.sourceInSync = false;
	next.statusInSync = false;
	next.endReportedId = null;
	next.echoUntilMs = nowMs + ECHO_WINDOW_MS;
	return [
		{
			type: "load",
			queueItemId: target.queueItemId,
			index: target.index,
			trackId: target.trackId,
		},
	];
}

function adjacentDirection(
	room: ListenTogetherRoom,
	index: number,
	queueItemId: string | null,
): "next" | "previous" | null {
	if (queueItemId === null) return null;
	const isAt = (at: number) => {
		const trackId = room.trackIds[at];
		return (
			trackId !== undefined && roomQueueItemId(at, trackId) === queueItemId
		);
	};
	if (isAt(index + 1)) return "next";
	if (isAt(index - 1)) return "previous";
	return null;
}

function planEnded(
	next: RoomSyncMemo,
	input: RoomSyncInput,
	target: NonNullable<ReturnType<typeof roomTarget>>,
	expectedMs: number | null,
): RoomSyncAction[] {
	const { room, local, nowMs } = input;
	if (next.endReportedId !== target.queueItemId) {
		if (!room.playing) return [];
		next.endReportedId = target.queueItemId;
		next.commandPending = true;
		return [
			{
				type: "command",
				command: {
					type: "skip",
					direction: "next",
					fromTrackId: target.trackId,
				},
			},
		];
	}
	// The Room stayed on this Track and went back into it, so play it again.
	const isBackInTrack =
		expectedMs !== null && expectedMs < local.durationMs - END_MARGIN_MS;
	if (!isBackInTrack) return [];
	next.loadingId = target.queueItemId;
	next.loadStartedAtMs = nowMs;
	next.needsInitialSeek = true;
	next.statusInSync = false;
	next.endReportedId = null;
	next.echoUntilMs = nowMs + ECHO_WINDOW_MS;
	return [
		{
			type: "load",
			queueItemId: target.queueItemId,
			index: target.index,
			trackId: target.trackId,
		},
	];
}

function correctStatus(
	next: RoomSyncMemo,
	nowMs: number,
	shouldPlay: boolean,
): RoomSyncAction[] {
	if (nowMs - next.lastCorrectionAtMs < CORRECTION_RETRY_MS) return [];
	next.lastCorrectionAtMs = nowMs;
	next.echoUntilMs = nowMs + ECHO_WINDOW_MS;
	return [{ type: shouldPlay ? "play" : "pause" }];
}

function planPosition(
	previous: RoomSyncMemo,
	next: RoomSyncMemo,
	input: RoomSyncInput,
	expectedMs: number | null,
	wasStatusInSync: boolean,
): RoomSyncAction[] {
	const { room, local, nowMs } = input;
	const hasBaseline =
		previous.lastPositionMs !== null &&
		previous.lastPositionId === local.queueItemId;
	const roomJumped = hasRoomJumped(previous, nowMs, expectedMs);
	if (roomJumped) {
		next.cooldownUntilMs = 0;
		next.consecutiveSeeks = 0;
	}

	if (
		!roomJumped &&
		hasBaseline &&
		wasStatusInSync &&
		nowMs >= next.echoUntilMs &&
		hasLocalJump(previous, local, nowMs)
	) {
		next.commandPending = true;
		return [
			{
				type: "command",
				command: { type: "seek", positionMs: Math.round(local.positionMs) },
			},
		];
	}

	if (expectedMs === null) return [];
	const isNearEnd =
		local.durationMs > 0 && expectedMs >= local.durationMs - END_MARGIN_MS;
	const currentMs = projectedPositionMs(local, nowMs);
	const isDrifting = Math.abs(currentMs - expectedMs) > DRIFT_THRESHOLD_MS;

	if (next.needsInitialSeek) {
		next.needsInitialSeek = false;
		if (!isDrifting || isNearEnd) return [];
		return seekTo(next, nowMs, expectedMs);
	}
	if (roomJumped) {
		return isDrifting && !isNearEnd ? seekTo(next, nowMs, expectedMs) : [];
	}
	if (nowMs < next.cooldownUntilMs) return [];
	// Both engines report "playing" before audio starts: only a playhead that
	// moves can be measured. A paused one stays where it was put.
	const isMeasurable =
		!room.playing ||
		(hasBaseline && local.positionMs > (previous.lastPositionMs ?? 0));
	if (!isMeasurable) return [];
	if (!isDrifting) {
		next.consecutiveSeeks = 0;
		return [];
	}
	return isNearEnd ? [] : seekTo(next, nowMs, expectedMs);
}

/** The Room's playhead moved other than by time passing: someone sought. */
function hasRoomJumped(
	previous: RoomSyncMemo,
	nowMs: number,
	expectedMs: number | null,
): boolean {
	if (expectedMs === null || previous.lastExpectedMs === null) return false;
	const elapsedMs = nowMs - previous.lastTickAtMs;
	const movedMs = expectedMs - previous.lastExpectedMs;
	return (
		movedMs < -DRIFT_THRESHOLD_MS || movedMs > elapsedMs + DRIFT_THRESHOLD_MS
	);
}

/**
 * A stalled playhead stands still, which never counts: only a move forward
 * past what time explains, or a move back.
 */
function hasLocalJump(
	previous: RoomSyncMemo,
	local: LocalPlayback,
	nowMs: number,
): boolean {
	const movedMs = local.positionMs - (previous.lastPositionMs ?? 0);
	const elapsedMs = isAudible(local.status) ? nowMs - previous.lastTickAtMs : 0;
	return (
		movedMs < -LOCAL_SEEK_THRESHOLD_MS ||
		movedMs > elapsedMs + LOCAL_SEEK_THRESHOLD_MS
	);
}

/** Engines report position a few times a second; bring it up to now. */
function projectedPositionMs(local: LocalPlayback, nowMs: number): number {
	if (!isAudible(local.status)) return local.positionMs;
	return local.positionMs + Math.max(0, nowMs - local.observedAtMs);
}

function seekTo(
	next: RoomSyncMemo,
	nowMs: number,
	positionMs: number,
): RoomSyncAction[] {
	const cooldownMs = Math.min(
		SEEK_COOLDOWN_MS * 2 ** next.consecutiveSeeks,
		MAX_SEEK_COOLDOWN_MS,
	);
	next.consecutiveSeeks += 1;
	next.cooldownUntilMs = nowMs + cooldownMs;
	next.echoUntilMs = nowMs + ECHO_WINDOW_MS;
	return [{ type: "seek", positionMs: Math.round(positionMs) }];
}
