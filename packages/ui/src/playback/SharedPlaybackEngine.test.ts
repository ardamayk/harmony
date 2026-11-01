import type { PlaybackState, PlaybackStateCommand } from "@repo/api-client";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import type { PlaybackSessionState, PlaybackSource } from "./PlaybackEngine";
import { SharedPlaybackEngine } from "./SharedPlaybackEngine";
import type { SharedPlaybackTransport } from "./SharedPlaybackEngine";
import { roomQueueItemId } from "../listen-together/roomSync";
import { InMemoryPlaybackEngine } from "./testing/InMemoryPlaybackEngine";

type TrackSource = Extract<PlaybackSource, { type: "track" }>;

const trackSource: TrackSource = {
	type: "track",
	track: {
		id: "track-1",
		title: "Track 1",
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120_000,
		format: "flac",
		genres: [],
	},
	playbackUrl: "/stream/track-1",
	queueItemId: "item-1",
};

const nextTrackSource: TrackSource = {
	...trackSource,
	track: { ...trackSource.track, id: "track-2", title: "Track 2" },
	playbackUrl: "/stream/track-2",
	queueItemId: "item-2",
};

const roomSource: TrackSource = {
	...trackSource,
	queueItemId: roomQueueItemId(0, "track-1"),
};

function sharedState(overrides: Partial<PlaybackState> = {}): PlaybackState {
	return {
		revision: 1,
		queueItemId: "item-1",
		trackId: "track-1",
		playing: false,
		positionMs: 0,
		positionAtMs: 0,
		activeDeviceId: null,
		activeDeviceName: null,
		remoteCommandRevision: 0,
		skipRequest: null,
		serverTimeMs: 0,
		...overrides,
	};
}

class FakeTransport implements SharedPlaybackTransport {
	device = { id: "device-1", name: "Firefox on Linux" };
	failHandshake = false;
	states: PlaybackState[] = [];
	commands: PlaybackStateCommand[] = [];
	resolved: PlaybackState[] = [];
	private listeners = new Set<(state: PlaybackState) => void>();

	async getDevice() {
		if (this.failHandshake) throw new Error("offline");
		return this.device;
	}

	async getState() {
		if (this.failHandshake) throw new Error("offline");
		return this.states.at(-1) ?? sharedState();
	}

	async sendCommand(command: PlaybackStateCommand) {
		this.commands.push(command);
		const base = this.states.at(-1) ?? sharedState();
		if (command.type === "claim") {
			// A claim always succeeds and starts a new revision.
			const answer: PlaybackState = {
				...base,
				revision: base.revision + 1,
				queueItemId: command.queueItemId,
				trackId: command.trackId,
				positionMs: command.positionMs,
				playing: command.playing,
				remoteCommandRevision: base.remoteCommandRevision,
				skipRequest: null,
				activeDeviceId: this.device.id,
				activeDeviceName: this.device.name,
			};
			this.states.push(answer);
			return answer;
		}
		if (command.type === "report") {
			const answer: PlaybackState = {
				...base,
				queueItemId: command.queueItemId,
				trackId: command.trackId,
				positionMs: command.positionMs,
				playing: command.playing,
			};
			this.states.push(answer);
			return answer;
		}
		return base;
	}

	subscribe(onState: (state: PlaybackState) => void) {
		this.listeners.add(onState);
		return () => this.listeners.delete(onState);
	}

	async resolveSource(state: PlaybackState) {
		this.resolved.push(state);
		if (!state.trackId) return null;
		return state.trackId === nextTrackSource.track.id
			? nextTrackSource
			: trackSource;
	}

	emit(state: PlaybackState) {
		this.states.push(state);
		for (const listener of this.listeners) listener(state);
	}
}

type Harness = {
	engine: SharedPlaybackEngine;
	local: InMemoryPlaybackEngine;
	transport: FakeTransport;
	state: () => PlaybackSessionState;
};

function createHarness(options?: {
	device?: { id: string; name: string };
	localReports?: boolean;
	/** Set before construction, so the first handshake fails. */
	failHandshake?: boolean;
	/** Snapshots the handshake reads: pushed before construction. */
	initialStates?: PlaybackState[];
}): Harness {
	const local = new InMemoryPlaybackEngine();
	if (options?.localReports) {
		Object.defineProperty(local, "reportsSharedPlaybackState", {
			value: true,
		});
	}
	const transport = new FakeTransport();
	if (options?.device) transport.device = options.device;
	transport.failHandshake = options?.failHandshake ?? false;
	transport.states.push(...(options?.initialStates ?? []));
	const engine = new SharedPlaybackEngine(local, transport);
	// PlaybackProvider lifts this on mounting, outside a Room.
	engine.setSharedPlaybackSuspended(false);
	return { engine, local, transport, state: () => engine.getState() };
}

/** Runs the handshake and the following resolve through the microtask queue. */
async function settle(times = 12) {
	for (let index = 0; index < times; index += 1) await Promise.resolve();
}

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(() => {
	vi.useRealTimers();
});

describe("SharedPlaybackEngine before the server answers", () => {
	it("is a plain pass-through, so an older server behaves as before", async () => {
		const { engine, transport } = createHarness({
			failHandshake: true,
		});

		await engine.play(trackSource);
		expect(engine.getState().source).toEqual(trackSource);
		expect(engine.getState().status).toBe("playing");

		const navigations: string[] = [];
		engine.subscribeNavigation?.((direction) => navigations.push(direction));
		engine.next();
		expect(navigations).toEqual(["next"]);

		engine.setVolume(0.5);
		expect(engine.getState().volume).toBe(0.5);
		expect(transport.commands).toHaveLength(0);
		engine.destroy();
	});

	it("keeps the pass-through after handshake failures", async () => {
		const { engine } = createHarness({ failHandshake: true });
		vi.advanceTimersByTime(1000);
		await settle();

		await engine.play(trackSource);
		expect(engine.getState().status).toBe("playing");
		expect(engine.getState().sharedPlayback).toBeUndefined();
		engine.destroy();
	});

	it("claims once the handshake lands after a play started blind", async () => {
		const { engine, local, transport, state } = createHarness();
		// Play before the handshake answered: the sound is already out here.
		await engine.play(trackSource);

		transport.states.push(
			sharedState({
				playing: false,
				positionMs: 30_000,
				activeDeviceId: "device-2",
				activeDeviceName: "Phone",
			}),
		);
		await settle();

		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toMatchObject({
			deviceId: "device-1",
			queueItemId: "item-1",
			trackId: "track-1",
			positionMs: 0,
			playing: true,
		});
		expect(local.getState().status).toBe("playing");
		expect(state().sharedPlayback).toBeUndefined();
		engine.destroy();
	});
});

describe("SharedPlaybackEngine following", () => {
	it("mirrors the shared snapshot: Track, playhead and status", async () => {
		const { engine, transport, state } = createHarness();
		transport.states.push(
			sharedState({
				playing: true,
				positionMs: 10_000,
				positionAtMs: 100_000,
				serverTimeMs: 100_000,
				activeDeviceId: "device-2",
				activeDeviceName: "Harmony on desktop",
			}),
		);
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				playing: true,
				positionMs: 10_000,
				positionAtMs: 100_000,
				serverTimeMs: 100_000,
				activeDeviceId: "device-2",
				activeDeviceName: "Harmony on desktop",
			}),
		);

		const session = state();
		expect(session.source?.type).toBe("track");
		expect(session.source?.type === "track" && session.source.queueItemId).toBe(
			"item-1",
		);
		expect(session.status).toBe("playing");
		expect(session.currentTime).toBe(10);
		expect(session.duration).toBe(120);
		expect(session.sharedPlayback).toEqual({
			followingDeviceName: "Harmony on desktop",
		});

		// The playhead advances on the 1 s timer without new snapshots.
		vi.advanceTimersByTime(3000);
		expect(state().currentTime).toBeCloseTo(13, 5);
		engine.destroy();
	});

	it("reads a stale snapshot as paused at its playhead", async () => {
		const { engine, transport, state } = createHarness();
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				playing: true,
				positionMs: 5000,
				positionAtMs: 0,
				serverTimeMs: 0,
				activeDeviceId: "device-2",
				activeDeviceName: "Harmony on desktop",
			}),
		);
		await settle();

		// The report is 25 seconds old: past the 20 s liveness window.
		vi.advanceTimersByTime(25_000);
		expect(state().status).toBe("paused");
		expect(state().currentTime).toBe(5);
		engine.destroy();
	});

	it("keeps the local volume and shuffle while following", async () => {
		const { engine, transport, local, state } = createHarness();
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				activeDeviceId: "device-2",
				activeDeviceName: "Other",
			}),
		);
		await settle();

		engine.setVolume(0.3);
		engine.toggleShuffle();
		expect(state().volume).toBe(0.3);
		expect(state().shuffleEnabled).toBe(true);
		expect(local.getState().volume).toBe(0.3);
		engine.destroy();
	});

	it("sends pause, seek and skip commands to the Active Device", async () => {
		const { engine, transport } = createHarness();
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				playing: true,
				positionMs: 10_000,
				positionAtMs: 100_000,
				serverTimeMs: 100_000,
				activeDeviceId: "device-2",
				activeDeviceName: "Other",
			}),
		);
		await settle();

		engine.pause();
		engine.seek(30);
		engine.next();
		expect(transport.commands).toEqual([
			{ type: "pause" },
			{ type: "seek", positionMs: 30_000 },
			{ type: "skip", direction: "next" },
		]);
		engine.destroy();
	});

	it("takes over paused at the neighbouring item when nothing is playing", async () => {
		const { engine, local, transport, state } = createHarness();
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				playing: false,
				positionMs: 4000,
				queueItemId: "item-1",
				trackId: "track-1",
				activeDeviceId: "device-2",
				activeDeviceName: "Other",
			}),
		);
		await settle();
		engine.setSharedQueueSources([trackSource, nextTrackSource]);

		engine.next();
		await settle();

		const session = state();
		expect(session.source).toEqual(nextTrackSource);
		expect(session.status).toBe("paused");
		expect(session.sharedPlayback).toBeUndefined();
		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toEqual({
			type: "claim",
			deviceId: "device-1",
			deviceName: "Firefox on Linux",
			queueItemId: "item-2",
			trackId: "track-2",
			positionMs: 0,
			playing: false,
		});
		// The silent local Player stays unloaded until this device plays.
		expect(local.getState().source).toBeNull();
		engine.destroy();
	});

	it("plays a takeover paused at the shared playhead", async () => {
		const { engine, local, transport } = createHarness();
		await settle();
		transport.emit(
			sharedState({
				revision: 2,
				playing: false,
				positionMs: 8000,
				queueItemId: "item-1",
				trackId: "track-1",
				activeDeviceId: "device-2",
				activeDeviceName: "Other",
			}),
		);
		await settle();

		await engine.play();
		await settle();

		expect(local.getState().source).toMatchObject({
			type: "track",
			startPositionSeconds: 8,
		});
		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toMatchObject({ playing: true, positionMs: 8000 });
		engine.destroy();
	});
});

describe("SharedPlaybackEngine active", () => {
	it("passes controls through and claims on play without awaiting it", async () => {
		const { engine, local, transport } = createHarness();
		await settle();
		// The server has another device active; this claim takes over.
		transport.states.push(sharedState({ activeDeviceId: "device-2" }));

		await engine.play(trackSource);

		expect(local.getState().source).toEqual(trackSource);
		expect(local.getState().status).toBe("playing");
		expect(engine.getState().sharedPlayback).toBeUndefined();
		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toEqual({
			type: "claim",
			deviceId: "device-1",
			deviceName: "Firefox on Linux",
			queueItemId: "item-1",
			trackId: "track-1",
			positionMs: 0,
			playing: true,
		});
		engine.destroy();
	});

	it("reports play, pause, source change and jumps, then heartbeats", async () => {
		const { engine, local, transport } = createHarness();
		await engine.play(trackSource);
		await settle();

		expect(transport.commands.some((command) => command.type === "claim")).toBe(
			true,
		);

		local.pause();
		expect(
			transport.commands.some(
				(command) =>
					command.type === "report" &&
					command.playing === false &&
					command.positionMs === 0,
			),
		).toBe(true);

		// No heartbeat while paused.
		const reportsWhilePaused = transport.commands.filter(
			(command) => command.type === "report",
		).length;
		vi.advanceTimersByTime(20_000);
		expect(
			transport.commands.filter((command) => command.type === "report"),
		).toHaveLength(reportsWhilePaused);

		void local.play();
		local.seek(40);
		expect(
			transport.commands.some(
				(command) =>
					command.type === "report" &&
					command.playing === true &&
					command.positionMs === 40_000,
			),
		).toBe(true);

		// While playing the heartbeat reports every 5 seconds.
		const reportsAfterJump = transport.commands.filter(
			(command) => command.type === "report",
		).length;
		vi.advanceTimersByTime(5000);
		expect(
			transport.commands.filter((command) => command.type === "report").length,
		).toBeGreaterThan(reportsAfterJump);
		engine.destroy();
	});

	it("obeys a remote pause and a remote seek", async () => {
		const { engine, local, transport } = createHarness();
		await engine.play(trackSource);
		await settle();

		transport.emit(
			sharedState({
				revision: 3,
				playing: false,
				positionMs: 12_000,
				remoteCommandRevision: 1,
				activeDeviceId: "device-1",
			}),
		);
		expect(local.getState().status).toBe("paused");

		transport.emit(
			sharedState({
				revision: 4,
				playing: false,
				positionMs: 30_000,
				remoteCommandRevision: 2,
				activeDeviceId: "device-1",
			}),
		);
		expect(local.getState().currentTime).toBe(30);

		// A newer snapshot without a new command does nothing again.
		transport.emit(
			sharedState({
				revision: 5,
				playing: false,
				positionMs: 30_000,
				remoteCommandRevision: 2,
				activeDeviceId: "device-1",
			}),
		);
		expect(local.getState().currentTime).toBe(30);
		engine.destroy();
	});

	it("carries out a skip request once, on the Player", async () => {
		const { engine, local, transport } = createHarness();
		await engine.play(trackSource);
		await settle();
		const navigations: string[] = [];
		local.subscribeNavigation((direction) => navigations.push(direction));

		const skip = sharedState({
			revision: 3,
			playing: true,
			activeDeviceId: "device-1",
			skipRequest: { revision: 9, direction: "next" },
		});
		transport.emit(skip);
		transport.emit({ ...skip, revision: 4 });

		expect(navigations).toEqual(["next"]);
		engine.destroy();
	});

	it("stops the local Player and follows when another device becomes Active", async () => {
		const { engine, local, transport, state } = createHarness();
		await engine.play(trackSource);
		await settle();

		transport.emit(
			sharedState({
				revision: 3,
				playing: true,
				positionMs: 15_000,
				positionAtMs: Date.now(),
				serverTimeMs: Date.now(),
				activeDeviceId: "device-2",
				activeDeviceName: "Phone",
			}),
		);

		await settle();
		expect(local.getState().source).toBeNull();
		expect(state().status).toBe("playing");
		expect(state().sharedPlayback).toEqual({ followingDeviceName: "Phone" });
		engine.destroy();
	});

	it("follows the truth inside a playback_device_not_active answer", async () => {
		const { engine, local, transport, state } = createHarness();
		await engine.play(trackSource);
		await settle();

		// The report 409 resolves with the conflict state: another device won.
		vi.spyOn(transport, "sendCommand").mockImplementationOnce(async () => {
			transport.states.push(
				sharedState({
					revision: 9,
					playing: true,
					activeDeviceId: "device-2",
					activeDeviceName: "Phone",
				}),
			);
			return transport.states.at(-1)!;
		});
		local.pause();

		await settle();
		expect(local.getState().source).toBeNull();
		expect(state().sharedPlayback).toEqual({ followingDeviceName: "Phone" });
		engine.destroy();
	});

	it("claims with null ids for a source outside the library", async () => {
		const radioSource: PlaybackSource = {
			type: "radio-station",
			station: {
				id: "station-1",
				name: "Station",
				streamUrl: "https://example.com/live",
				tags: [],
				source: "manual",
				isFavorite: false,
				position: 0,
			},
			playbackUrl: "/radio/station-1",
			sourceUrl: "https://example.com/live",
		};
		const { engine, transport } = createHarness();
		await settle();
		transport.states.push(sharedState());
		await engine.play(radioSource);

		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toMatchObject({
			queueItemId: null,
			trackId: null,
			playing: true,
		});
		engine.destroy();
	});
});

describe("SharedPlaybackEngine Desktop hand-off", () => {
	it("claims and follows but leaves reporting and obeying to the local engine", async () => {
		const { engine, local, transport } = createHarness({
			localReports: true,
		});
		await settle();

		await engine.play(trackSource);
		local.pause();
		transport.emit(
			sharedState({
				revision: 3,
				playing: false,
				remoteCommandRevision: 1,
				activeDeviceId: "device-1",
			}),
		);

		// No reports of its own, and no pause obeyed twice: the core does both.
		expect(
			transport.commands.every((command) => command.type !== "report"),
		).toBe(true);
		expect(local.getState().status).toBe("paused");
		engine.destroy();
	});
});

describe("SharedPlaybackEngine startup with a restored Player", () => {
	it("stays Active on the same item and seeks to the shared playhead", async () => {
		const { engine, local } = createHarness({
			initialStates: [
				sharedState({
					playing: true,
					positionMs: 10_000,
					positionAtMs: 1000,
					serverTimeMs: 1000,
					activeDeviceId: "device-1",
				}),
			],
		});
		await local.play(trackSource);
		local.seek(60);
		await settle();

		// 60 s locally against 10 s shared: the shared playhead wins.
		expect(local.getState().currentTime).toBe(10);
		expect(engine.getState().status).toBe("playing");
		expect(engine.getState().sharedPlayback).toBeUndefined();
		engine.destroy();
	});

	it("goes silent and follows when another device is Active", async () => {
		const { engine, local, state } = createHarness({
			initialStates: [
				sharedState({
					playing: true,
					activeDeviceId: "device-2",
					activeDeviceName: "Phone",
				}),
			],
		});
		await local.play(trackSource);
		local.seek(60);
		await settle();

		expect(local.getState().source).toBeNull();
		expect(state().sharedPlayback).toEqual({ followingDeviceName: "Phone" });
		engine.destroy();
	});

	it("claims an empty shared state so the server holds this playback", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ queueItemId: null, trackId: null })],
		});
		await local.play(trackSource);
		local.seek(12);
		await settle();

		const claim = transport.commands.find(
			(command) => command.type === "claim",
		);
		expect(claim).toMatchObject({
			queueItemId: "item-1",
			trackId: "track-1",
			positionMs: 12_000,
			playing: true,
		});
		engine.destroy();
	});
});

describe("SharedPlaybackEngine suspended", () => {
	it("passes through without reporting or following", async () => {
		const { engine, local, transport, state } = createHarness();
		await settle();
		await engine.play(trackSource);
		await settle();

		engine.setSharedPlaybackSuspended(true);
		expect(engine.getState().source).toEqual(trackSource);
		expect(engine.getState().sharedPlayback).toBeUndefined();

		// A snapshot with a remote pause arrives; nothing is sent or obeyed.
		const commandsBefore = transport.commands.length;
		transport.emit(
			sharedState({
				revision: 3,
				playing: false,
				remoteCommandRevision: 1,
				activeDeviceId: "device-1",
			}),
		);
		vi.advanceTimersByTime(20_000);
		expect(transport.commands).toHaveLength(commandsBefore);
		expect(local.getState().status).toBe("playing");

		engine.setVolume(0.4);
		expect(state().volume).toBe(0.4);
		engine.destroy();
	});

	it("resumes following the shared state after unsuspends", async () => {
		const { engine, local, transport, state } = createHarness();
		await settle();
		await engine.play(trackSource);
		await settle();
		engine.setSharedPlaybackSuspended(true);

		transport.states.push(
			sharedState({
				revision: 5,
				playing: true,
				positionMs: 3000,
				positionAtMs: Date.now(),
				serverTimeMs: Date.now(),
				activeDeviceId: "device-2",
				activeDeviceName: "Phone",
			}),
		);
		engine.setSharedPlaybackSuspended(false);
		await settle();

		expect(local.getState().source).toBeNull();
		expect(state().sharedPlayback).toEqual({ followingDeviceName: "Phone" });
		engine.destroy();
	});
});

describe("SharedPlaybackEngine after a Room", () => {
	const empty = sharedState({ queueItemId: null, trackId: null });

	/** A Room that ends with its Track paused in the local Player. */
	async function endRoom({ engine, local }: Harness) {
		await settle();
		engine.setSharedPlaybackSuspended(true);
		await local.play(roomSource);
		local.pause();
		engine.setSharedPlaybackSuspended(false);
		await settle();
	}

	it("does not claim an empty state with the Room's Track", async () => {
		const harness = createHarness({ initialStates: [empty] });
		await endRoom(harness);

		expect(harness.transport.commands).toEqual([]);
		expect(harness.local.getState().source).toBeNull();
		expect(harness.state().source).toBeNull();
		harness.engine.destroy();
	});

	it("does not claim over the state with a blind start's Room Track", async () => {
		const harness = createHarness({ failHandshake: true });
		harness.engine.setSharedPlaybackSuspended(true);
		await harness.local.play(roomSource);
		harness.engine.setSharedPlaybackSuspended(false);
		await harness.engine.play(roomSource);
		harness.transport.failHandshake = false;
		vi.advanceTimersByTime(1000);
		await settle();

		expect(harness.transport.commands).toEqual([]);
		harness.engine.destroy();
	});

	it("never claims or reports a Room's Track from any path", async () => {
		const { engine, transport } = createHarness();
		await engine.play(trackSource);
		await settle();
		transport.commands = [];

		await engine.play(roomSource);
		vi.advanceTimersByTime(5000);
		await settle();
		engine.stop();
		await settle();

		expect(transport.commands).toEqual([]);
		engine.destroy();
	});

	it("leaves the state to the Desktop that was Active, while its core still stops", async () => {
		const harness = createHarness({
			localReports: true,
			initialStates: [
				sharedState({
					positionMs: 7000,
					activeDeviceId: "device-1",
					activeDeviceName: "Harmony on desktop",
				}),
			],
		});
		const { engine, local, transport, state } = harness;
		const stop = vi.spyOn(local, "stop").mockImplementation(() => undefined);
		await endRoom(harness);

		// A tick from the core, which has not stopped the Room's Track yet.
		local.seek(3);
		await settle();

		expect(stop).toHaveBeenCalledOnce();
		expect(state().source).toEqual(trackSource);
		expect(state().status).toBe("paused");
		expect(state().currentTime).toBe(7);

		engine.togglePlay();
		await settle();

		expect(transport.commands).toEqual([
			expect.objectContaining({
				type: "claim",
				queueItemId: "item-1",
				trackId: "track-1",
				positionMs: 7000,
			}),
		]);
		engine.destroy();
	});
});

describe("SharedPlaybackEngine concurrency", () => {
	const otherDevice = { activeDeviceId: "device-2", activeDeviceName: "Phone" };

	function deferredClaims(transport: FakeTransport) {
		const release: Array<() => void> = [];
		const fail: Array<(error: Error) => void> = [];
		const send = transport.sendCommand.bind(transport);
		vi.spyOn(transport, "sendCommand").mockImplementation((command) =>
			command.type === "claim"
				? new Promise((resolve, reject) => {
						transport.commands.push(command);
						release.push(() => {
							transport.commands.pop();
							resolve(send(command));
						});
						fail.push(reject);
					})
				: send(command),
		);
		return { release, fail };
	}

	it("obeys a remote pause that only the report's answer carries", async () => {
		const { engine, local, transport } = createHarness();
		await engine.play(trackSource);
		await settle();
		const pausedElsewhere = sharedState({
			revision: 20,
			playing: false,
			positionMs: 0,
			remoteCommandRevision: 20,
			activeDeviceId: "device-1",
		});
		// The heartbeat was based on an older revision: the server changes
		// nothing and answers with the pause, ahead of its frame.
		vi.spyOn(transport, "sendCommand").mockResolvedValueOnce(pausedElsewhere);

		vi.advanceTimersByTime(5000);
		await settle();
		expect(local.getState().status).toBe("paused");

		// The frame arriving late changes nothing and pauses nothing twice.
		const pause = vi.spyOn(local, "pause");
		transport.emit(pausedElsewhere);
		expect(pause).not.toHaveBeenCalled();
		engine.destroy();
	});

	it("sends no report while a claim is out, so a 409 cannot silence the Player", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ playing: true, ...otherDevice })],
		});
		await settle();
		const { release } = deferredClaims(transport);

		await engine.play(trackSource);
		local.seek(40);
		vi.advanceTimersByTime(5000);
		await settle();

		expect(
			transport.commands.filter((command) => command.type === "report"),
		).toEqual([]);
		release[0]();
		await settle();
		expect(local.getState().status).toBe("playing");
		engine.destroy();
	});

	it("ignores older snapshots during a claim, then obeys a newer one held back", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ playing: true, ...otherDevice })],
		});
		await settle();
		const { release } = deferredClaims(transport);
		await engine.play(trackSource);

		// A heartbeat of the old Active Device races the claim.
		transport.emit(sharedState({ revision: 2, playing: true, ...otherDevice }));
		expect(local.getState().status).toBe("playing");
		expect(engine.getState().sharedPlayback).toBeUndefined();

		// A snapshot older than the claim's answer still loses after it.
		transport.states.pop();
		release[0]();
		await settle();
		expect(local.getState().status).toBe("playing");

		transport.emit(sharedState({ revision: 1, playing: true, ...otherDevice }));
		expect(local.getState().status).toBe("playing");
		engine.destroy();
	});

	it("retries a failed claim while nobody else has started", async () => {
		const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ playing: true, ...otherDevice })],
		});
		await settle();
		const { fail } = deferredClaims(transport);
		await engine.play(trackSource);
		fail[0](new Error("400 trackId is not in the library"));
		await settle();

		transport.emit(sharedState({ revision: 5, playing: true, ...otherDevice }));
		vi.advanceTimersByTime(5000);
		await settle();

		expect(local.getState().status).toBe("playing");
		expect(warn).toHaveBeenCalledOnce();
		expect(
			transport.commands.filter((command) => command.type === "report"),
		).toEqual([]);
		expect(
			transport.commands.filter((command) => command.type === "claim"),
		).toHaveLength(2);
		engine.destroy();
		warn.mockRestore();
	});

	it("gives way after a failed claim when another device has started", async () => {
		const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ playing: true, ...otherDevice })],
		});
		await settle();
		const { fail } = deferredClaims(transport);
		await engine.play(trackSource);
		fail[0](new Error("503"));
		await settle();

		transport.states.push(
			sharedState({
				revision: 5,
				playing: true,
				activeDeviceId: "device-3",
				activeDeviceName: "Tablet",
			}),
		);
		vi.advanceTimersByTime(5000);
		await settle();

		expect(local.getState().status).toBe("idle");
		expect(
			transport.commands.filter((command) => command.type === "claim"),
		).toHaveLength(1);
		engine.destroy();
		warn.mockRestore();
	});

	it("keeps Radio playing when another window opens on the same device", async () => {
		const local = new InMemoryPlaybackEngine();
		const radio = {
			type: "radio-station",
			station: { id: "station-1" },
			playbackUrl: "/radio",
			sourceUrl: "/radio",
		} as unknown as PlaybackSource;
		await local.play(radio);
		const transport = new FakeTransport();
		transport.states.push(
			sharedState({
				queueItemId: null,
				trackId: null,
				playing: true,
				activeDeviceId: transport.device.id,
				activeDeviceName: transport.device.name,
			}),
		);
		const engine = new SharedPlaybackEngine(local, transport);
		engine.setSharedPlaybackSuspended(false);
		await settle();

		expect(local.getState().status).toBe("playing");
		engine.destroy();
	});

	it("starts a Track that played to its end from the beginning", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ positionMs: 119_900, ...otherDevice })],
		});
		await settle();
		await engine.play();
		await settle();

		expect(local.getState().source).toMatchObject({ startPositionSeconds: 0 });
		expect(transport.commands.at(0)).toMatchObject({
			type: "claim",
			positionMs: 0,
		});
		engine.destroy();
	});

	it("corrects a claim that said playing when the start was blocked", async () => {
		const { engine, local, transport } = createHarness();
		await settle();
		const { release } = deferredClaims(transport);
		await engine.play(trackSource);
		// What a browser's NotAllowedError leaves behind.
		local.pause();
		release[0]();
		await settle();

		expect(transport.commands.at(-1)).toMatchObject({
			type: "report",
			playing: false,
		});
		engine.destroy();
	});

	it("never starts a Following tab by itself", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [sharedState({ playing: true, ...otherDevice })],
		});
		const play = vi.spyOn(local, "play");
		await settle();

		transport.emit(sharedState({ revision: 2, playing: true, ...otherDevice }));
		vi.advanceTimersByTime(30_000);
		await settle();
		transport.emit(
			sharedState({ revision: 3, playing: false, activeDeviceId: "device-1" }),
		);
		await settle();

		expect(play).not.toHaveBeenCalled();
		engine.destroy();
	});

	it("takes over a dead Active Device at its last playhead on togglePlay", async () => {
		const { engine, local, transport } = createHarness({
			initialStates: [
				sharedState({
					playing: true,
					positionMs: 5000,
					positionAtMs: Date.now(),
					serverTimeMs: Date.now(),
					...otherDevice,
				}),
			],
		});
		await settle();
		vi.advanceTimersByTime(25_000);

		engine.togglePlay();
		await settle();

		expect(transport.commands).toContainEqual(
			expect.objectContaining({
				type: "claim",
				positionMs: 5000,
				playing: true,
			}),
		);
		expect(local.getState().status).toBe("playing");
		engine.destroy();
	});

	it("follows its own Track after a reload, with nothing loaded here", async () => {
		const { engine, local, state } = createHarness({
			initialStates: [
				sharedState({
					positionMs: 7000,
					activeDeviceId: "device-1",
					activeDeviceName: "Firefox on Linux",
				}),
			],
		});
		await settle();

		expect(state().source).toEqual(trackSource);
		expect(state().status).toBe("paused");
		expect(state().currentTime).toBe(7);
		expect(state().sharedPlayback).toEqual({ followingDeviceName: null });
		expect(local.getState().source).toBeNull();
		engine.destroy();
	});

	describe("after a reload of the tab that was playing", () => {
		const reloaded = () =>
			createHarness({
				initialStates: [
					sharedState({
						playing: true,
						positionMs: 7000,
						positionAtMs: Date.now(),
						serverTimeMs: Date.now(),
						activeDeviceId: "device-1",
						activeDeviceName: "Firefox on Linux",
					}),
				],
			});

		it("reads as paused at the shared playhead, since nobody can obey", async () => {
			const { engine, state } = reloaded();
			await settle();
			vi.advanceTimersByTime(3000);

			expect(state().source).toEqual(trackSource);
			expect(state().status).toBe("paused");
			expect(state().currentTime).toBe(7);
			expect(state().sharedPlayback).toEqual({ followingDeviceName: null });
			engine.destroy();
		});

		it("takes the sound over with the first press of the main button", async () => {
			const { engine, local, transport } = reloaded();
			await settle();
			vi.advanceTimersByTime(3000);

			engine.togglePlay();
			await settle();

			expect(transport.commands).toEqual([
				expect.objectContaining({
					type: "claim",
					positionMs: 7000,
					playing: true,
				}),
			]);
			expect(local.getState().status).toBe("playing");
			engine.destroy();
		});

		it("moves to the next Track paused instead of sending a lost skip", async () => {
			const { engine, local, transport, state } = reloaded();
			await settle();
			engine.setSharedQueueSources([trackSource, nextTrackSource]);

			engine.next();
			await settle();

			expect(transport.commands).toEqual([
				expect.objectContaining({
					type: "claim",
					queueItemId: "item-2",
					positionMs: 0,
					playing: false,
				}),
			]);
			expect(state().source).toEqual(nextTrackSource);
			expect(state().status).toBe("paused");
			expect(local.getState().source).toBeNull();
			engine.destroy();
		});
	});

	it("names the other device only while it really plays", async () => {
		const { engine, transport, state } = createHarness();
		await settle();
		transport.emit(sharedState({ revision: 2, ...otherDevice }));

		expect(state().sharedPlayback).toEqual({ followingDeviceName: null });
		engine.destroy();
	});

	it("reports the Track at the playhead with playing false on stop", async () => {
		const { engine, local, transport } = createHarness();
		await engine.play(trackSource);
		await settle();
		local.seek(9);

		engine.stop();
		await settle();

		const reports = transport.commands.filter(
			(command) => command.type === "report",
		);
		expect(reports.at(-1)).toMatchObject({
			trackId: "track-1",
			positionMs: 9000,
			playing: false,
		});
		engine.destroy();
	});

	it("leaves a Desktop core's Player for the core to stop", async () => {
		const { engine, local, transport, state } = createHarness({
			localReports: true,
		});
		await engine.play(trackSource);
		await settle();

		transport.emit(sharedState({ revision: 9, playing: true, ...otherDevice }));

		expect(local.getState().source).toEqual(trackSource);
		expect(state().sharedPlayback).toEqual({ followingDeviceName: "Phone" });
		engine.destroy();
	});

	it("stays paused on a further skip after taking over", async () => {
		const { engine, local, transport } = createHarness();
		await settle();
		transport.emit(sharedState({ revision: 2, ...otherDevice }));
		await settle();
		const third = { ...nextTrackSource, queueItemId: "item-3" };
		engine.setSharedQueueSources([trackSource, nextTrackSource, third]);

		engine.next();
		await settle();
		engine.next();
		await settle();

		expect(engine.getState().source).toEqual(third);
		expect(local.getState().source).toBeNull();
		engine.destroy();
	});

	it("resolves only the latest followed item", async () => {
		const { engine, transport, state } = createHarness();
		await settle();
		const resolvers: Array<(source: TrackSource | null) => void> = [];
		vi.spyOn(transport, "resolveSource").mockImplementation(
			() =>
				new Promise<TrackSource | null>((resolve) => {
					resolvers.push(resolve);
				}),
		);
		transport.emit(
			sharedState({ revision: 2, queueItemId: "a", ...otherDevice }),
		);
		transport.emit(
			sharedState({ revision: 3, queueItemId: "b", ...otherDevice }),
		);

		resolvers[1](nextTrackSource);
		resolvers[0](trackSource);
		await settle();

		expect(state().source).toEqual(nextTrackSource);
		engine.destroy();
	});
});

describe("SharedPlaybackEngine compatibility", () => {
	it("mirrors the local engine's Queue shape, which decides who advances the Queue", () => {
		const local = new InMemoryPlaybackEngine();
		const engine = new SharedPlaybackEngine(local, new FakeTransport());
		expect(engine.syncQueueContext).toBeUndefined();
		expect(engine.subscribeNavigation).toBeDefined();
		engine.destroy();
	});

	it("stays a pass-through for good when the capability is missing", async () => {
		const local = new InMemoryPlaybackEngine();
		const transport = new FakeTransport();
		const isSupported = vi.fn(async () => false);
		const engine = new SharedPlaybackEngine(local, transport, isSupported);
		engine.setSharedPlaybackSuspended(false);
		await settle();

		await engine.play(trackSource);
		vi.advanceTimersByTime(120_000);
		await settle();

		expect(isSupported).toHaveBeenCalledOnce();
		expect(transport.commands).toEqual([]);
		expect(local.getState().status).toBe("playing");
		engine.destroy();
	});

	it("tells the local engine about a suspension even when it starts suspended", () => {
		const local = new InMemoryPlaybackEngine();
		const setSuspended = vi.fn();
		Object.assign(local, { setSharedPlaybackSuspended: setSuspended });
		const engine = new SharedPlaybackEngine(local, new FakeTransport());

		engine.setSharedPlaybackSuspended(true);

		expect(setSuspended).toHaveBeenCalledWith(true);
		engine.destroy();
	});

	it("does nothing until the provider lifts the suspension", async () => {
		const local = new InMemoryPlaybackEngine();
		const transport = new FakeTransport();
		transport.states.push(
			sharedState({
				playing: true,
				activeDeviceId: "device-2",
				activeDeviceName: "Phone",
			}),
		);
		const engine = new SharedPlaybackEngine(local, transport);
		await local.play(trackSource);
		await settle();

		expect(local.getState().status).toBe("playing");
		expect(engine.getState().sharedPlayback).toBeUndefined();
		engine.destroy();
	});
});
