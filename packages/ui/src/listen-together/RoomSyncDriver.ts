import type {
	ListenTogetherCommand,
	ListenTogetherRoom,
} from "@repo/api-client";
import type {
	PlaybackEngine,
	PlaybackSessionState,
	PlaybackSource,
	RepeatMode,
} from "../playback/PlaybackEngine";
import {
	type ClockSample,
	estimateClockOffset,
	INITIAL_ROOM_SYNC_MEMO,
	type LocalPlayback,
	planRoomSync,
	type RoomSyncAction,
	type RoomSyncMemo,
	roomQueueItemId,
	settleCommand,
	settleLoad,
	SYNC_TICK_MS,
} from "./roomSync";

const INITIAL_CLOCK_SAMPLES = 5;
const REFRESH_CLOCK_SAMPLES = 3;
const CLOCK_REFRESH_MS = 60_000;
const CLOCK_RETRY_MS = 2000;
/** Cycling the Repeat Mode this many times from a mode reaches "off". */
const REPEAT_CYCLES_TO_OFF: Record<RepeatMode, number> = {
	off: 0,
	once: 2,
	loop: 1,
};

export type RoomSyncDriverOptions = {
	engine: PlaybackEngine;
	getClock: () => Promise<{ serverTimeMs: number }>;
	/** The Playback Source of one place in the Room Queue. */
	resolveSource: (
		trackId: string,
		queueItemId: string,
	) => Promise<PlaybackSource>;
	/** Sends a Room command and adopts the answer. Never rejects. */
	sendCommand: (command: ListenTogetherCommand) => Promise<void>;
	now?: () => number;
};

type SavedPlayerSettings = {
	playbackRate: number;
	shuffleEnabled: boolean;
	repeatMode: RepeatMode;
};

/**
 * Keeps one PlaybackEngine on the Room State (ADR 0037). It feeds the pure
 * plan in roomSync.ts and carries out its actions through the engine
 * interface, so the audio element and mpv follow the same code.
 */
export class RoomSyncDriver {
	private readonly now: () => number;
	private memo: RoomSyncMemo = INITIAL_ROOM_SYNC_MEMO;
	private room: ListenTogetherRoom | null = null;
	private clockOffsetMs: number | null = null;
	private tickTimer: ReturnType<typeof setInterval> | null = null;
	private clockTimer: ReturnType<typeof setTimeout> | null = null;
	private unsubscribe: (() => void) | null = null;
	private isRunning = false;
	private saved: SavedPlayerSettings | null = null;
	private observedPositionMs = 0;
	private observedAtMs = 0;

	constructor(private readonly options: RoomSyncDriverOptions) {
		this.now = options.now ?? Date.now;
	}

	/** Starts following. Starting again after `suspend` is safe. */
	start(room: ListenTogetherRoom): void {
		if (this.isRunning) return;
		this.isRunning = true;
		this.room = room;
		this.prepareEngine();
		this.observe(this.options.engine.getState());
		this.unsubscribe = this.options.engine.subscribe((state) =>
			this.observe(state),
		);
		this.tickTimer = setInterval(this.tick, SYNC_TICK_MS);
		void this.refreshClock(INITIAL_CLOCK_SAMPLES);
		this.tick();
	}

	update(room: ListenTogetherRoom): void {
		this.room = room;
		this.tick();
	}

	/** Stops the timers but keeps what `finish` restores. */
	suspend(): void {
		this.isRunning = false;
		if (this.tickTimer !== null) clearInterval(this.tickTimer);
		if (this.clockTimer !== null) clearTimeout(this.clockTimer);
		this.tickTimer = null;
		this.clockTimer = null;
		this.unsubscribe?.();
		this.unsubscribe = null;
	}

	/**
	 * This client's part in the Room is over. A Guest's playback stops; the
	 * Owner's Player stays where it is, paused.
	 */
	finish(ending: "stop" | "pause"): void {
		this.suspend();
		const { engine } = this.options;
		if (ending === "stop") engine.stop();
		else engine.pause();
		this.restoreEngine();
	}

	private prepareEngine() {
		if (this.saved) return;
		const { engine } = this.options;
		const state = engine.getState();
		this.saved = {
			playbackRate: state.playbackRate ?? 1,
			shuffleEnabled: state.shuffleEnabled,
			repeatMode: state.repeatMode,
		};
		// Rate stays 1: correction is by seeking only, and a Player that repeats
		// or shuffles would leave the Room Queue on its own.
		if (this.saved.playbackRate !== 1) engine.setPlaybackRate?.(1);
		if (state.shuffleEnabled) engine.toggleShuffle();
		for (
			let cycle = 0;
			cycle < REPEAT_CYCLES_TO_OFF[state.repeatMode];
			cycle++
		) {
			engine.cycleRepeatMode();
		}
		this.holdAtTrackEnd();
	}

	private restoreEngine() {
		const saved = this.saved;
		if (!saved) return;
		this.saved = null;
		const { engine } = this.options;
		if (engine.syncQueueContext) engine.setStopAfterCurrent?.(false);
		if (saved.playbackRate !== 1) engine.setPlaybackRate?.(saved.playbackRate);
		if (saved.shuffleEnabled) engine.toggleShuffle();
		const cycles =
			saved.repeatMode === "once" ? 1 : saved.repeatMode === "loop" ? 2 : 0;
		for (let cycle = 0; cycle < cycles; cycle++) engine.cycleRepeatMode();
	}

	/**
	 * An engine that owns its Queue advances by itself at the end of a Track.
	 * Holding it there leaves the next Track to the Room. It also knows the
	 * Tracks around the current one, so OS controls can still move it, which
	 * sync reads as a skip. The flag lasts one Track end, so every load sets it.
	 */
	private holdAtTrackEnd() {
		const { engine } = this.options;
		if (engine.syncQueueContext) engine.setStopAfterCurrent?.(true);
	}

	private observe(state: PlaybackSessionState) {
		const positionMs = state.currentTime * 1000;
		if (positionMs === this.observedPositionMs) return;
		this.observedPositionMs = positionMs;
		this.observedAtMs = this.now();
	}

	private readLocal(): LocalPlayback {
		const state = this.options.engine.getState();
		return {
			queueItemId:
				state.source?.type === "track"
					? (state.source.queueItemId ?? null)
					: null,
			hasSource: state.source !== null,
			status: state.status,
			positionMs: state.currentTime * 1000,
			observedAtMs: this.observedAtMs,
			durationMs: state.duration * 1000,
		};
	}

	private readonly tick = () => {
		if (!this.isRunning || !this.room) return;
		const { memo, actions } = planRoomSync(this.memo, {
			room: this.room,
			clockOffsetMs: this.clockOffsetMs,
			local: this.readLocal(),
			nowMs: this.now(),
		});
		this.memo = memo;
		for (const action of actions) this.perform(action);
	};

	private perform(action: RoomSyncAction) {
		const { engine, sendCommand } = this.options;
		switch (action.type) {
			case "load":
				void this.load(action);
				return;
			case "play":
				// A refusal shows in the engine's state.
				void engine.play().catch(() => undefined);
				return;
			case "pause":
				engine.pause();
				return;
			case "seek":
				engine.seek(action.positionMs / 1000);
				return;
			case "command":
				void sendCommand(action.command).finally(() => {
					this.memo = settleCommand(this.memo);
					this.tick();
				});
		}
	}

	private async load(action: Extract<RoomSyncAction, { type: "load" }>) {
		const { engine, resolveSource } = this.options;
		let succeeded = true;
		try {
			const source = await resolveSource(action.trackId, action.queueItemId);
			if (this.memo.loadingId !== action.queueItemId) return;
			if (engine.syncQueueContext) {
				const context = await this.queueContext(action, source);
				await engine.syncQueueContext(context.sources, context.currentIndex);
				this.holdAtTrackEnd();
			}
			await engine.play(source);
		} catch (error) {
			succeeded = false;
			console.warn("Failed to load the Room's Track", {
				trackId: action.trackId,
				error,
			});
		}
		this.memo = settleLoad(this.memo, action.queueItemId, succeeded);
		this.tick();
	}

	private async queueContext(
		action: Extract<RoomSyncAction, { type: "load" }>,
		source: PlaybackSource,
	) {
		const [previous, next] = await Promise.all(
			[action.index - 1, action.index + 1].map((index) =>
				this.neighbourSource(index),
			),
		);
		return {
			sources: [previous, source, next].filter(
				(entry): entry is PlaybackSource => entry !== null,
			),
			currentIndex: previous ? 1 : 0,
		};
	}

	private async neighbourSource(index: number) {
		const trackId = this.room?.trackIds[index];
		if (trackId === undefined) return null;
		return this.options
			.resolveSource(trackId, roomQueueItemId(index, trackId))
			.catch(() => null);
	}

	private async refreshClock(sampleCount: number) {
		const samples: ClockSample[] = [];
		for (let sample = 0; sample < sampleCount && this.isRunning; sample++) {
			const sentAtMs = this.now();
			try {
				const { serverTimeMs } = await this.options.getClock();
				samples.push({ sentAtMs, receivedAtMs: this.now(), serverTimeMs });
			} catch (error) {
				console.warn("Failed to read the Music Server's clock", { error });
				break;
			}
		}
		if (!this.isRunning) return;
		this.clockOffsetMs = estimateClockOffset(samples) ?? this.clockOffsetMs;
		const hasEstimate = this.clockOffsetMs !== null;
		this.clockTimer = setTimeout(
			() =>
				void this.refreshClock(
					hasEstimate ? REFRESH_CLOCK_SAMPLES : INITIAL_CLOCK_SAMPLES,
				),
			hasEstimate ? CLOCK_REFRESH_MS : CLOCK_RETRY_MS,
		);
	}
}
