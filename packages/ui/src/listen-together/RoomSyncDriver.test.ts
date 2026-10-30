import type {
	ListenTogetherCommand,
	ListenTogetherRoom,
} from "@repo/api-client";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import type { PlaybackSource } from "../playback/PlaybackEngine";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import { roomQueueItemId, SYNC_TICK_MS } from "./roomSync";
import { RoomSyncDriver } from "./RoomSyncDriver";

const START_MS = 1_000_000;
const SETTLE_MS = 3000;

function createTrack(id: string) {
	return {
		id,
		title: id,
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120_000,
		format: "flac",
		genres: [],
	};
}

function createRoom(
	overrides: Partial<ListenTogetherRoom> = {},
): ListenTogetherRoom {
	return {
		revision: 1,
		trackIds: ["track-1", "track-2", "track-3"],
		currentIndex: 0,
		playing: true,
		positionMs: 0,
		positionAtMs: START_MS,
		lyricsOpen: false,
		guestCount: 1,
		maxGuests: 8,
		expiresAt: "2026-10-31T12:00:00Z",
		...overrides,
	};
}

/** An engine with a Queue of its own, as the Desktop Client's is. */
class NativeQueueEngine extends InMemoryPlaybackEngine {
	contexts: { sources: PlaybackSource[]; currentIndex: number | null }[] = [];
	async syncQueueContext(
		sources: PlaybackSource[],
		currentIndex: number | null,
	) {
		this.contexts.push({ sources, currentIndex });
	}
}

function setup(engine = new InMemoryPlaybackEngine()) {
	const commands: ListenTogetherCommand[] = [];
	let driver: RoomSyncDriver | null = null;
	let room = createRoom();
	const applyCommand = (command: ListenTogetherCommand) => {
		commands.push(command);
	};
	const instance = new RoomSyncDriver({
		engine,
		getClock: async () => ({ serverTimeMs: Date.now() }),
		resolveSource: async (trackId, queueItemId) => ({
			type: "track",
			track: createTrack(trackId),
			playbackUrl: `/stream/${trackId}`,
			queueItemId,
		}),
		sendCommand: async (command) => applyCommand(command),
	});
	driver = instance;
	return {
		engine,
		commands,
		driver: instance,
		start(next = room) {
			room = next;
			driver?.start(next);
		},
		snapshot(next: Partial<ListenTogetherRoom>) {
			room = { ...room, ...next, revision: room.revision + 1 };
			driver?.update(room);
		},
	};
}

/** Lets time pass with the Player's playhead following it. */
async function play(engine: InMemoryPlaybackEngine, durationMs: number) {
	for (let elapsed = 0; elapsed < durationMs; elapsed += SYNC_TICK_MS) {
		if (engine.getState().status === "playing") {
			engine.seek(engine.getState().currentTime + SYNC_TICK_MS / 1000);
		}
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);
	}
}

describe("RoomSyncDriver", () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.setSystemTime(START_MS);
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("plays the Room's Track through the engine", async () => {
		const { engine, start } = setup();

		start();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		const source = engine.getState().source;
		expect(source?.type === "track" && source.track.id).toBe("track-1");
		expect(source?.type === "track" && source.queueItemId).toBe(
			roomQueueItemId(0, "track-1"),
		);
		expect(engine.getState().status).toBe("playing");
	});

	it("joins a Track already under way at the expected playhead", async () => {
		const { engine, start } = setup();

		start(createRoom({ positionMs: 30_000 }));
		await play(engine, 1000);

		expect(engine.getState().currentTime).toBeGreaterThan(30);
	});

	it("keeps the speed at 1 and leaves the rest off during a Room", async () => {
		const engine = new InMemoryPlaybackEngine({
			playbackRate: 1.5,
			repeatMode: "once",
			shuffleEnabled: true,
		});
		const { start, driver } = setup(engine);

		start();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		expect(engine.getState().playbackRate).toBe(1);
		expect(engine.getState().repeatMode).toBe("off");
		expect(engine.getState().shuffleEnabled).toBe(false);

		driver.finish("pause");

		expect(engine.getState().playbackRate).toBe(1.5);
		expect(engine.getState().repeatMode).toBe("once");
		expect(engine.getState().shuffleEnabled).toBe(true);
	});

	it("follows a pause from the Room without sending a command back", async () => {
		const { engine, commands, start, snapshot } = setup();
		start();
		await play(engine, SETTLE_MS);

		snapshot({ playing: false, positionMs: 3000, positionAtMs: Date.now() });
		await play(engine, SETTLE_MS);

		expect(engine.getState().status).toBe("paused");
		expect(commands).toEqual([]);
	});

	it("turns a pause from media keys into one command, without a loop", async () => {
		const { engine, commands, start, snapshot } = setup();
		start();
		await play(engine, SETTLE_MS);

		// Media Session and MPRIS reach the engine without passing the UI.
		engine.pause();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);
		expect(commands).toEqual([{ type: "pause" }]);

		snapshot({ playing: false, positionMs: 3000, positionAtMs: Date.now() });
		await play(engine, SETTLE_MS);

		expect(commands).toEqual([{ type: "pause" }]);
		expect(engine.getState().status).toBe("paused");
	});

	it("turns a seek from media keys into one command, without a loop", async () => {
		const { engine, commands, start, snapshot } = setup();
		start();
		await play(engine, SETTLE_MS);

		engine.seek(60);
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);
		expect(commands).toEqual([{ type: "seek", positionMs: 60_000 }]);

		snapshot({ positionMs: 60_000, positionAtMs: Date.now() });
		await play(engine, SETTLE_MS);

		expect(commands).toHaveLength(1);
		expect(engine.getState().currentTime).toBeGreaterThan(60);
	});

	it("sends a command that never takes hold only once per change", async () => {
		const { engine, commands, start } = setup();
		start();
		await play(engine, SETTLE_MS);

		engine.pause();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);
		// No snapshot followed: the Room still plays, so the Player goes back.
		await play(engine, SETTLE_MS);

		expect(commands).toEqual([{ type: "pause" }]);
		expect(engine.getState().status).toBe("playing");
	});

	it("reports the end of a Track instead of moving on", async () => {
		const { engine, commands, start } = setup();
		start(createRoom({ positionMs: 119_000 }));
		await play(engine, 1000);

		engine.finish();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		expect(commands).toEqual([
			{ type: "skip", direction: "next", fromTrackId: "track-1" },
		]);
		const source = engine.getState().source;
		expect(source?.type === "track" && source.track.id).toBe("track-1");
	});

	it("changes Track when the snapshot arrives", async () => {
		const { engine, start, snapshot } = setup();
		start();
		await play(engine, SETTLE_MS);

		snapshot({ currentIndex: 1, positionMs: 0, positionAtMs: Date.now() });
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		const source = engine.getState().source;
		expect(source?.type === "track" && source.track.id).toBe("track-2");
	});

	it("does not restart a load when snapshots repeat", async () => {
		const { engine, start, snapshot } = setup();
		const play = vi.spyOn(engine, "play");
		start();

		snapshot({ guestCount: 2 });
		snapshot({ guestCount: 3 });
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		expect(play).toHaveBeenCalledTimes(1);
	});

	it("corrects drift by seeking, never through the speed", async () => {
		const { engine, start } = setup();
		const setPlaybackRate = vi.spyOn(engine, "setPlaybackRate");
		start();
		await play(engine, SETTLE_MS);
		setPlaybackRate.mockClear();

		// Holds back for a second, as a stall does.
		await vi.advanceTimersByTimeAsync(1000);
		await play(engine, 1000);

		expect(setPlaybackRate).not.toHaveBeenCalled();
		const position = engine.getState().currentTime;
		const expected = (Date.now() - START_MS) / 1000;
		expect(Math.abs(position - expected)).toBeLessThan(0.5);
	});

	it("stops a Guest's playback when the Room ends", async () => {
		const { engine, start, driver } = setup();
		start();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		driver.finish("stop");

		expect(engine.getState().status).toBe("idle");
		expect(engine.getState().source).toBeNull();
	});

	it("leaves the Owner's Player paused when the Room ends", async () => {
		const { engine, start, driver } = setup();
		start();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		driver.finish("pause");

		expect(engine.getState().status).toBe("paused");
		expect(engine.getState().source).not.toBeNull();
	});

	it("stops following once suspended", async () => {
		const { engine, start, driver } = setup();
		start();
		await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

		driver.suspend();
		engine.pause();
		await vi.advanceTimersByTimeAsync(SETTLE_MS);

		expect(engine.getState().status).toBe("paused");
	});

	describe("with an engine that owns its Queue", () => {
		it("gives it the Tracks around the current one and holds it at the end", async () => {
			const engine = new NativeQueueEngine();
			const { start } = setup(engine);

			start(createRoom({ currentIndex: 1 }));
			await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

			expect(engine.contexts.at(-1)?.currentIndex).toBe(1);
			expect(
				engine.contexts
					.at(-1)
					?.sources.map(
						(source) => source.type === "track" && source.queueItemId,
					),
			).toEqual([
				roomQueueItemId(0, "track-1"),
				roomQueueItemId(1, "track-2"),
				roomQueueItemId(2, "track-3"),
			]);
			expect(engine.getState().stopAfterCurrent).toBe(true);
		});

		it("reads an OS Next that moved the engine as a skip", async () => {
			const engine = new NativeQueueEngine();
			const { start, commands } = setup(engine);
			start();
			await play(engine, SETTLE_MS);

			// MPRIS next: the core plays the adjacent source by itself.
			await engine.play({
				type: "track",
				track: createTrack("track-2"),
				playbackUrl: "/stream/track-2",
				queueItemId: roomQueueItemId(1, "track-2"),
			});
			await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

			expect(commands).toEqual([
				{ type: "skip", direction: "next", fromTrackId: "track-1" },
			]);
		});

		it("gives up the hold when the Room ends", async () => {
			const engine = new NativeQueueEngine();
			const { start, driver } = setup(engine);
			start();
			await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

			driver.finish("pause");

			expect(engine.getState().stopAfterCurrent).toBe(false);
		});

		it("does not hold an engine that advances through the provider", async () => {
			const { engine, start } = setup();
			start();
			await vi.advanceTimersByTimeAsync(SYNC_TICK_MS);

			expect(engine.getState().stopAfterCurrent).toBeFalsy();
		});
	});
});
