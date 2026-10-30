import type { ListenTogetherRoom } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import {
	CORRECTION_RETRY_MS,
	DRIFT_THRESHOLD_MS,
	estimateClockOffset,
	expectedPositionMs,
	INITIAL_ROOM_SYNC_MEMO,
	isStaleSnapshot,
	type LocalPlayback,
	LOAD_TIMEOUT_MS,
	planRoomSync,
	type RoomSyncMemo,
	roomQueueItemId,
	SEEK_COOLDOWN_MS,
	settleCommand,
	settleLoad,
	SYNC_TICK_MS,
} from "./roomSync";

const TRACKS = ["track-a", "track-b", "track-c"];

function createRoom(overrides: Partial<ListenTogetherRoom> = {}) {
	return {
		revision: 1,
		trackIds: TRACKS,
		currentIndex: 1,
		playing: true,
		positionMs: 10_000,
		positionAtMs: 100_000,
		lyricsOpen: false,
		guestCount: 1,
		maxGuests: 8,
		expiresAt: "2026-10-31T12:00:00Z",
		...overrides,
	} satisfies ListenTogetherRoom;
}

const CURRENT_ID = roomQueueItemId(1, "track-b");

function createLocal(overrides: Partial<LocalPlayback> = {}): LocalPlayback {
	return {
		queueItemId: CURRENT_ID,
		hasSource: true,
		status: "playing",
		positionMs: 10_000,
		observedAtMs: 100_000,
		durationMs: 200_000,
		...overrides,
	};
}

/**
 * Runs sync as the driver does, with the Player reporting a fresh position
 * on every tick, and collects what it asked for.
 */
function createSim(initial: { memo?: Partial<RoomSyncMemo> } = {}) {
	let memo: RoomSyncMemo = { ...INITIAL_ROOM_SYNC_MEMO, ...initial.memo };
	return {
		get memo() {
			return memo;
		},
		update(change: (current: RoomSyncMemo) => RoomSyncMemo) {
			memo = change(memo);
		},
		step(
			room: ListenTogetherRoom,
			local: LocalPlayback,
			nowMs: number,
			clockOffsetMs: number | null = 0,
		) {
			const result = planRoomSync(memo, { room, local, nowMs, clockOffsetMs });
			memo = result.memo;
			return result.actions;
		},
	};
}

/** A Player that shows the Room State and is exactly on time. */
function settled(sim: ReturnType<typeof createSim>, room = createRoom()) {
	sim.step(
		room,
		createLocal({ positionMs: 9750, observedAtMs: 99_750 }),
		99_750,
	);
	const actions = sim.step(room, createLocal(), 100_000);
	expect(actions).toEqual([]);
	return room;
}

describe("expectedPositionMs", () => {
	it("advances from the anchor while playing", () => {
		expect(expectedPositionMs(createRoom(), 103_000)).toBe(13_000);
	});

	it("holds the anchor while paused", () => {
		expect(expectedPositionMs(createRoom({ playing: false }), 103_000)).toBe(
			10_000,
		);
	});

	it("never goes before the anchor", () => {
		expect(expectedPositionMs(createRoom(), 90_000)).toBe(10_000);
	});
});

describe("isStaleSnapshot", () => {
	it("ignores only revisions older than the newest", () => {
		const room = createRoom({ revision: 4 });
		expect(isStaleSnapshot(room, null)).toBe(false);
		expect(isStaleSnapshot(room, 3)).toBe(false);
		expect(isStaleSnapshot(room, 4)).toBe(false);
		expect(isStaleSnapshot(room, 5)).toBe(true);
	});
});

describe("estimateClockOffset", () => {
	it("uses the round-trip midpoint of the fastest sample", () => {
		expect(
			estimateClockOffset([
				{ sentAtMs: 1000, receivedAtMs: 1400, serverTimeMs: 6000 },
				{ sentAtMs: 2000, receivedAtMs: 2020, serverTimeMs: 7010 },
				{ sentAtMs: 3000, receivedAtMs: 3100, serverTimeMs: 8000 },
			]),
		).toBe(5000);
	});

	it("is null without samples", () => {
		expect(estimateClockOffset([])).toBeNull();
	});
});

describe("loading the Room's Track", () => {
	it("loads the current Track when the Player is elsewhere", () => {
		const sim = createSim();
		const actions = sim.step(
			createRoom(),
			createLocal({ queueItemId: null, hasSource: false, status: "idle" }),
			100_000,
		);
		expect(actions).toEqual([
			{ type: "load", queueItemId: CURRENT_ID, index: 1, trackId: "track-b" },
		]);
	});

	it("does not restart a load when snapshots repeat", () => {
		const sim = createSim();
		const local = createLocal({
			queueItemId: null,
			hasSource: false,
			status: "idle",
		});
		sim.step(createRoom(), local, 100_000);

		expect(sim.step(createRoom({ revision: 2 }), local, 100_250)).toEqual([]);
		expect(sim.step(createRoom({ revision: 3 }), local, 100_500)).toEqual([]);
	});

	it("loads the next Track at once when the Room moves during a load", () => {
		const sim = createSim();
		const local = createLocal({
			queueItemId: null,
			hasSource: false,
			status: "idle",
		});
		sim.step(createRoom(), local, 100_000);

		const actions = sim.step(
			createRoom({ revision: 2, currentIndex: 2 }),
			local,
			100_250,
		);

		expect(actions[0]).toMatchObject({ type: "load", trackId: "track-c" });
	});

	it("tries again after a load that never settled", () => {
		const sim = createSim();
		const local = createLocal({
			queueItemId: null,
			hasSource: false,
			status: "idle",
		});
		sim.step(createRoom(), local, 100_000);

		const actions = sim.step(
			createRoom(),
			local,
			100_000 + LOAD_TIMEOUT_MS + 1,
		);

		expect(actions[0]).toMatchObject({ type: "load" });
	});

	it("does not reload a Track that failed here until the Room moves on", () => {
		const sim = createSim();
		const local = createLocal({
			queueItemId: null,
			hasSource: false,
			status: "idle",
		});
		sim.step(createRoom(), local, 100_000);
		sim.update((memo) => settleLoad(memo, CURRENT_ID, false));

		expect(sim.step(createRoom(), local, 100_250)).toEqual([]);
		expect(
			sim.step(createRoom({ currentIndex: 2 }), local, 100_500)[0],
		).toMatchObject({ type: "load", trackId: "track-c" });
	});

	it("leaves a Player in error alone so the user can retry", () => {
		const sim = createSim();
		const actions = sim.step(
			createRoom(),
			createLocal({ status: "error" }),
			100_000,
		);
		expect(actions).toEqual([]);
		expect(sim.memo.failedId).toBe(CURRENT_ID);
	});

	it("moves to the expected playhead as soon as the load settles", () => {
		const sim = createSim();
		sim.step(
			createRoom(),
			createLocal({ queueItemId: null, hasSource: false, status: "idle" }),
			100_000,
		);
		sim.update((memo) => settleLoad(memo, CURRENT_ID, true));

		const actions = sim.step(
			createRoom(),
			createLocal({ positionMs: 0, observedAtMs: 101_000 }),
			101_000,
		);

		expect(actions).toEqual([{ type: "seek", positionMs: 11_000 }]);
	});

	it("silences a Player with no Track to follow", () => {
		const sim = createSim();
		expect(
			sim.step(
				createRoom({ currentIndex: null, playing: false }),
				createLocal({ queueItemId: "personal", status: "playing" }),
				100_000,
			),
		).toEqual([{ type: "pause" }]);
	});

	it("tells two places of one Track apart", () => {
		expect(roomQueueItemId(0, "t")).not.toBe(roomQueueItemId(1, "t"));
	});
});

describe("following play and pause", () => {
	it("pauses when the Room pauses, without a command", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom({ revision: 2, playing: false, positionMs: 10_250 }),
			createLocal({ positionMs: 10_250, observedAtMs: 100_250 }),
			100_250,
		);

		expect(actions).toEqual([{ type: "pause" }]);
	});

	it("resumes when the Room resumes", () => {
		const sim = createSim();
		const paused = createRoom({ playing: false });
		sim.step(paused, createLocal({ status: "paused" }), 99_750);
		sim.step(paused, createLocal({ status: "paused" }), 100_000);

		const actions = sim.step(
			createRoom({ revision: 2, positionAtMs: 100_250 }),
			createLocal({ status: "paused" }),
			100_250,
		);

		expect(actions).toEqual([{ type: "play" }]);
	});

	it("repeats a correction that has not shown, but not on every tick", () => {
		const sim = createSim();
		settled(sim);
		const paused = createRoom({ revision: 2, playing: false });

		expect(sim.step(paused, createLocal(), 100_250)).toEqual([
			{ type: "pause" },
		]);
		expect(sim.step(paused, createLocal(), 100_500)).toEqual([]);
		expect(
			sim.step(paused, createLocal(), 100_250 + CORRECTION_RETRY_MS),
		).toEqual([{ type: "pause" }]);
	});
});

describe("local changes become Room commands", () => {
	it("turns a pause sync did not make into a pause command", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({ status: "paused", positionMs: 10_250 }),
			100_250,
		);

		expect(actions).toEqual([{ type: "command", command: { type: "pause" } }]);
	});

	it("turns a play sync did not make into a resume command", () => {
		const sim = createSim();
		const paused = createRoom({ playing: false });
		sim.step(paused, createLocal({ status: "paused" }), 99_750);
		sim.step(paused, createLocal({ status: "paused" }), 100_000);

		const actions = sim.step(paused, createLocal(), 100_250);

		expect(actions).toEqual([{ type: "command", command: { type: "resume" } }]);
	});

	it("turns a large jump into a seek command", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({ positionMs: 60_000, observedAtMs: 100_250 }),
			100_250,
		);

		expect(actions).toEqual([
			{ type: "command", command: { type: "seek", positionMs: 60_000 } },
		]);
	});

	it("turns a jump back into a seek command", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({ positionMs: 2000, observedAtMs: 100_250 }),
			100_250,
		);

		expect(actions[0]).toMatchObject({
			type: "command",
			command: { type: "seek", positionMs: 2000 },
		});
	});

	it("turns the Player moving to the next Room Track into a skip", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({
				queueItemId: roomQueueItemId(2, "track-c"),
				positionMs: 0,
			}),
			100_250,
		);

		expect(actions).toEqual([
			{
				type: "command",
				command: {
					type: "skip",
					direction: "next",
					fromTrackId: "track-b",
					fromIndex: 1,
				},
			},
		]);
	});

	it("turns the Player moving to the previous Room Track into a skip", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({
				queueItemId: roomQueueItemId(0, "track-a"),
				positionMs: 0,
			}),
			100_250,
		);

		expect(actions[0]).toMatchObject({
			command: { type: "skip", direction: "previous" },
		});
	});

	it("pauses the Room when the Player is stopped from outside", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({
				queueItemId: null,
				hasSource: false,
				status: "idle",
				positionMs: 0,
			}),
			100_250,
		);

		expect(actions).toEqual([{ type: "command", command: { type: "pause" } }]);
	});

	it("waits for the command instead of correcting the Player", () => {
		const sim = createSim();
		settled(sim);
		sim.step(createRoom(), createLocal({ status: "paused" }), 100_250);

		expect(
			sim.step(createRoom(), createLocal({ status: "paused" }), 100_500),
		).toEqual([]);
		expect(
			sim.step(createRoom(), createLocal({ status: "paused" }), 100_750),
		).toEqual([]);
	});

	it("puts the Player back when the command is refused", () => {
		const sim = createSim();
		settled(sim);
		sim.step(createRoom(), createLocal({ status: "paused" }), 100_250);
		sim.update(settleCommand);

		const actions = sim.step(
			createRoom(),
			createLocal({ status: "paused" }),
			101_500,
		);

		expect(actions).toEqual([{ type: "play" }]);
	});

	it("does not echo the pause it just made", () => {
		const sim = createSim();
		settled(sim);
		const paused = createRoom({ revision: 2, playing: false });

		sim.step(paused, createLocal(), 100_250);
		// The engine reports the pause later; none of this may become a command.
		for (let nowMs = 100_500; nowMs < 103_000; nowMs += SYNC_TICK_MS) {
			const status = nowMs < 100_800 ? "playing" : "paused";
			const actions = sim.step(paused, createLocal({ status }), nowMs);
			expect(actions.filter((action) => action.type === "command")).toEqual([]);
		}
	});

	it("does not echo the resume it just made", () => {
		const sim = createSim();
		const paused = createRoom({ playing: false });
		sim.step(paused, createLocal({ status: "paused" }), 99_750);
		sim.step(paused, createLocal({ status: "paused" }), 100_000);
		const playing = createRoom({ revision: 2, positionAtMs: 100_250 });

		sim.step(playing, createLocal({ status: "paused" }), 100_250);
		const actions = sim.step(
			playing,
			createLocal({ observedAtMs: 100_500, positionMs: 10_250 }),
			100_500,
		);

		expect(actions).toEqual([]);
	});

	it("does not echo the seek it just made", () => {
		const sim = createSim();
		settled(sim);
		const moved = createRoom({
			revision: 2,
			positionMs: 90_000,
			positionAtMs: 100_250,
		});

		expect(
			sim.step(
				moved,
				createLocal({ positionMs: 10_250, observedAtMs: 100_250 }),
				100_250,
			),
		).toEqual([{ type: "seek", positionMs: 90_000 }]);
		// The jump shows on the next ticks and is not a local seek.
		for (let step = 1; step <= 8; step += 1) {
			const nowMs = 100_250 + step * SYNC_TICK_MS;
			const actions = sim.step(
				moved,
				createLocal({
					positionMs: 90_000 + step * SYNC_TICK_MS,
					observedAtMs: nowMs,
				}),
				nowMs,
			);
			expect(actions).toEqual([]);
		}
	});

	it("does not take a stalled playhead for a seek", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom(),
			createLocal({ positionMs: 10_000, observedAtMs: 100_000 }),
			130_000,
		);

		expect(actions).toEqual([]);
	});

	it("is no command when a snapshot brings the same change", () => {
		const sim = createSim();
		settled(sim);

		const actions = sim.step(
			createRoom({ revision: 2, playing: false }),
			createLocal({ status: "paused" }),
			100_250,
		);

		expect(actions).toEqual([]);
	});
});

describe("the end of a Track", () => {
	it("reports it once, with the Track that ended", () => {
		const sim = createSim();
		const room = createRoom({ positionMs: 199_900 });
		const ended = createLocal({ status: "ended", positionMs: 200_000 });

		expect(sim.step(room, ended, 100_000)).toEqual([
			{
				type: "command",
				command: {
					type: "skip",
					direction: "next",
					fromTrackId: "track-b",
					fromIndex: 1,
				},
			},
		]);
		sim.update(settleCommand);
		expect(sim.step(room, ended, 100_250)).toEqual([]);
	});

	it("does not report the end of a Track in a paused Room", () => {
		const sim = createSim();
		expect(
			sim.step(
				createRoom({ playing: false }),
				createLocal({ status: "ended" }),
				100_000,
			),
		).toEqual([]);
	});

	it("plays again when the Room seeks back into the Track", () => {
		const sim = createSim();
		settled(sim);
		const ended = createLocal({ status: "ended", positionMs: 200_000 });
		sim.step(createRoom(), ended, 100_250);
		sim.update(settleCommand);

		const actions = sim.step(
			createRoom({ revision: 2, positionMs: 5000, positionAtMs: 100_500 }),
			ended,
			100_500,
		);

		expect(actions[0]).toMatchObject({ type: "load", trackId: "track-b" });
	});
});

describe("drift", () => {
	it("seeks a playing Player that drifted past the threshold", () => {
		const sim = createSim();
		settled(sim);

		sim.step(
			createRoom(),
			createLocal({ positionMs: 9700, observedAtMs: 100_250 }),
			100_250,
		);
		const actions = sim.step(
			createRoom(),
			createLocal({
				positionMs: 10_500 - DRIFT_THRESHOLD_MS - 50,
				observedAtMs: 100_500,
			}),
			100_500,
		);

		expect(actions).toEqual([{ type: "seek", positionMs: 10_500 }]);
	});

	it("leaves drift inside the threshold alone", () => {
		const sim = createSim();
		settled(sim);

		expect(
			sim.step(
				createRoom(),
				createLocal({
					positionMs: 10_250 - DRIFT_THRESHOLD_MS + 50,
					observedAtMs: 100_250,
				}),
				100_250,
			),
		).toEqual([]);
	});

	it("measures nothing until the playhead moves", () => {
		const sim = createSim();
		sim.step(createRoom(), createLocal({ positionMs: 0 }), 100_000);
		sim.update((memo) => ({
			...memo,
			cooldownUntilMs: 0,
			needsInitialSeek: false,
		}));
		// "playing" with a playhead stuck at the start: audio has not begun.
		for (const nowMs of [100_250, 100_500, 100_750]) {
			expect(
				sim.step(
					createRoom(),
					createLocal({ positionMs: 0, observedAtMs: 100_000 }),
					nowMs,
				),
			).toEqual([]);
		}
	});

	it("does not seek again during the cooldown", () => {
		const sim = createSim();
		settled(sim);
		const behind = (nowMs: number) =>
			createLocal({ positionMs: nowMs - 92_000, observedAtMs: nowMs });

		sim.step(createRoom(), behind(100_250), 100_250);
		expect(sim.step(createRoom(), behind(100_500), 100_500)).toHaveLength(1);
		for (const nowMs of [100_750, 101_000, 102_000]) {
			expect(sim.step(createRoom(), behind(nowMs), nowMs)).toEqual([]);
		}
	});

	it("backs off when seeks do not hold", () => {
		const sim = createSim();
		settled(sim);
		const behind = (nowMs: number) =>
			createLocal({ positionMs: nowMs - 92_000, observedAtMs: nowMs });
		sim.step(createRoom(), behind(100_250), 100_250);
		expect(sim.step(createRoom(), behind(100_500), 100_500)).toHaveLength(1);

		const secondSeekAt = 100_500 + SEEK_COOLDOWN_MS;
		expect(
			sim.step(createRoom(), behind(secondSeekAt), secondSeekAt),
		).toHaveLength(1);

		const afterDoubledCooldown = secondSeekAt + 2 * SEEK_COOLDOWN_MS;
		const beforeIt = afterDoubledCooldown - 500;
		expect(sim.step(createRoom(), behind(beforeIt), beforeIt)).toEqual([]);
		expect(
			sim.step(
				createRoom(),
				behind(afterDoubledCooldown),
				afterDoubledCooldown,
			),
		).toHaveLength(1);
	});

	it("follows a Room seek even during the cooldown", () => {
		const sim = createSim();
		settled(sim);
		sim.step(
			createRoom(),
			createLocal({ positionMs: 9000, observedAtMs: 100_250 }),
			100_250,
		);

		const actions = sim.step(
			createRoom({ revision: 2, positionMs: 70_000, positionAtMs: 100_500 }),
			createLocal({ positionMs: 10_250, observedAtMs: 100_500 }),
			100_500,
		);

		expect(actions).toEqual([{ type: "seek", positionMs: 70_000 }]);
	});

	it("uses the clock offset", () => {
		const sim = createSim();
		sim.step(createRoom(), createLocal({ positionMs: 9750 }), 49_750, 50_000);
		sim.step(
			createRoom(),
			createLocal({ positionMs: 10_000, observedAtMs: 50_000 }),
			50_000,
			50_000,
		);
		expect(
			sim.step(
				createRoom(),
				createLocal({ positionMs: 10_250, observedAtMs: 50_250 }),
				50_250,
				50_000,
			),
		).toEqual([]);
	});

	it("does not seek without a clock estimate", () => {
		const sim = createSim();
		sim.step(createRoom(), createLocal({ positionMs: 0 }), 100_000, null);
		expect(
			sim.step(
				createRoom(),
				createLocal({ positionMs: 100, observedAtMs: 100_250 }),
				100_250,
				null,
			),
		).toEqual([]);
	});

	it("does not seek to the very end of a Track", () => {
		const sim = createSim();
		const room = createRoom({ positionMs: 199_900 });
		settled(sim);
		expect(
			sim.step(
				room,
				createLocal({ positionMs: 199_000, observedAtMs: 100_250 }),
				100_250,
			),
		).toEqual([]);
	});

	it("moves a paused Player that sits elsewhere", () => {
		const sim = createSim();
		const paused = createRoom({ playing: false, positionMs: 50_000 });
		sim.step(
			paused,
			createLocal({ status: "paused", positionMs: 50_000 }),
			99_000,
		);
		sim.step(
			paused,
			createLocal({ status: "paused", positionMs: 50_000 }),
			99_250,
		);

		const actions = sim.step(
			createRoom({ playing: false, positionMs: 80_000, revision: 2 }),
			createLocal({ status: "paused", positionMs: 50_000 }),
			99_500,
		);

		expect(actions).toEqual([{ type: "seek", positionMs: 80_000 }]);
	});
});
