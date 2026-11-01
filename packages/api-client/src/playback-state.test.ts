import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	createApiClient,
	createSharedQueueEventStream,
	type PlaybackState,
	type PlaybackStateCommand,
	SHARED_SESSION_CAPABILITY,
} from "./index";
import { parsePlaybackState, playbackConflictState } from "./playback-state";
import type { QueueEventSource } from "./queue-events";

const state: PlaybackState = {
	revision: 7,
	queueItemId: "item-1",
	trackId: "track-1",
	playing: true,
	positionMs: 1000,
	positionAtMs: 9000,
	activeDeviceId: "device-1",
	activeDeviceName: "Firefox on Linux",
	remoteCommandRevision: 3,
	skipRequest: null,
	serverTimeMs: 9500,
};

class FakeEventSource implements QueueEventSource {
	static instances: FakeEventSource[] = [];
	listeners = new Map<string, (event: MessageEvent<string>) => void>();
	closed = false;
	readyState = 1;
	onerror: ((event: Event) => void) | null = null;
	constructor(readonly url: string) {
		FakeEventSource.instances.push(this);
	}
	addEventListener(
		name: string,
		listener: (event: MessageEvent<string>) => void,
	) {
		this.listeners.set(name, listener);
	}
	close() {
		this.closed = true;
	}
	emit(name: string, data = "") {
		this.listeners.get(name)?.(new MessageEvent(name, { data }));
	}
}

function jsonResponse(body: unknown, status = 200) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

function healthWith(capabilities: string[]) {
	return { capabilities, playback: { autoSkipOnErrorSeconds: 5 } };
}

function createClient(transport: typeof fetch) {
	return createApiClient({
		baseUrl: "http://server",
		transport,
		eventSourceFactory: (url) => new FakeEventSource(url),
		validateResponses: false,
	});
}

describe("parsePlaybackState", () => {
	it("accepts a full snapshot", () => {
		expect(parsePlaybackState(JSON.stringify(state))).toEqual(state);
	});

	it("rejects a payload without the required values", () => {
		const broken = JSON.stringify({ ...state, playing: "yes" });
		expect(() => parsePlaybackState(broken)).toThrow(TypeError);
		expect(() => parsePlaybackState("not json")).toThrow(SyntaxError);
	});
});

describe("playbackConflictState", () => {
	it("returns the state inside a playback_device_not_active answer", () => {
		const failure = {
			status: 409,
			body: {
				error: "conflict",
				code: "playback_device_not_active",
				message: "Another device is active",
				state,
			},
		};
		expect(playbackConflictState(failure)).toEqual(state);
	});

	it("ignores other failures", () => {
		expect(
			playbackConflictState({
				status: 409,
				body: { code: "queue_revision_conflict" },
			}),
		).toBeUndefined();
		expect(playbackConflictState(new Error("offline"))).toBeUndefined();
		expect(playbackConflictState(undefined)).toBeUndefined();
	});
});

describe("Shared Playback State client", () => {
	it("reads the state from GET /api/v1/playback/state", async () => {
		const transport = vi.fn(async () => jsonResponse(state)) as never;
		const client = createClient(transport);

		await expect(client.getPlaybackState()).resolves.toEqual(state);
		const [url] = (transport as unknown as ReturnType<typeof vi.fn>).mock
			.calls[0];
		expect(url).toBe("http://server/api/v1/playback/state");
	});

	it("sends a command and answers the state", async () => {
		const transport = vi.fn(async () => jsonResponse(state)) as never;
		const client = createClient(transport);
		const command: PlaybackStateCommand = {
			type: "claim",
			deviceId: "device-1",
			deviceName: "Firefox on Linux",
			queueItemId: "item-1",
			trackId: "track-1",
			positionMs: 1000,
			playing: true,
		};

		await expect(client.sendPlaybackStateCommand(command)).resolves.toEqual(
			state,
		);
		const [url, init] = (transport as unknown as ReturnType<typeof vi.fn>).mock
			.calls[0];
		expect(url).toBe("http://server/api/v1/playback/state/commands");
		expect(init.method).toBe("POST");
		expect(JSON.parse(init.body)).toEqual(command);
	});

	it("resolves a playback_device_not_active answer with its state", async () => {
		const conflict = {
			error: "conflict",
			code: "playback_device_not_active",
			message: "Another device is active",
			state,
		};
		const client = createClient(
			vi.fn(async () => jsonResponse(conflict, 409)) as never,
		);

		await expect(
			client.sendPlaybackStateCommand({ type: "pause" }),
		).resolves.toEqual(state);
	});

	it("rejects command failures without a conflict state", async () => {
		const client = createClient(
			vi.fn(async () =>
				jsonResponse({ error: "x", code: "invalid", message: "bad" }, 400),
			) as never,
		);

		await expect(
			client.sendPlaybackStateCommand({ type: "seek", positionMs: -1 }),
		).rejects.toMatchObject({ status: 400 });
	});
});

/** Flushes enough microtasks for the capability check and stream open. */
async function flush(times = 8) {
	for (let index = 0; index < times; index += 1) await Promise.resolve();
}

describe("subscribePlaybackState", () => {
	beforeEach(() => {
		FakeEventSource.instances = [];
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("delivers frames over the shared stream, one connection with the Queue events", async () => {
		const client = createClient(
			vi.fn(async () =>
				jsonResponse(
					healthWith([
						"api.v1",
						"playback.queue-events.v1",
						SHARED_SESSION_CAPABILITY,
					]),
				),
			) as never,
		);
		const snapshots: PlaybackState[] = [];
		const queueEvents: unknown[] = [];
		const unsubscribeState = client.subscribePlaybackState((event) =>
			snapshots.push(event),
		);
		const unsubscribeQueue = client.subscribePlaybackQueueEvents((event) =>
			queueEvents.push(event),
		);
		await flush();

		expect(FakeEventSource.instances).toHaveLength(1);
		const source = FakeEventSource.instances[0];
		expect(source.url).toBe("http://server/api/v1/playback/queue/events");
		source.emit("playback-state", JSON.stringify(state));
		expect(snapshots).toEqual([state]);

		const queueEvent = {
			revision: "2",
			sequence: "5",
			invalidates: ["queue"],
		};
		source.emit("queue-invalidated", JSON.stringify(queueEvent));
		expect(queueEvents).toEqual([queueEvent]);

		unsubscribeState();
		unsubscribeQueue();
		expect(source.closed).toBe(true);
	});
});

describe("shared stream hub", () => {
	beforeEach(() => {
		FakeEventSource.instances = [];
	});

	it("opens a new connection when a proxy's error ends EventSource", async () => {
		vi.useFakeTimers();
		try {
			const stream = createSharedQueueEventStream({
				getBaseUrl: () => "http://server",
				eventSourceFactory: (url) => new FakeEventSource(url),
			});
			const states: PlaybackState[] = [];
			const detach = stream.addListener(
				"playback-state",
				(event) => states.push(parsePlaybackState(event.data)),
				() => undefined,
			);
			await vi.advanceTimersByTimeAsync(0);
			const first = FakeEventSource.instances[0];
			first.readyState = 2;
			first.onerror?.(new Event("error"));
			await vi.advanceTimersByTimeAsync(1000);

			expect(FakeEventSource.instances).toHaveLength(2);
			FakeEventSource.instances[1].emit(
				"playback-state",
				JSON.stringify(state),
			);
			expect(states).toHaveLength(1);
			detach();
		} finally {
			vi.useRealTimers();
		}
	});

	it("opens with the first listener, fans out per name, closes with the last", async () => {
		const stream = createSharedQueueEventStream({
			getBaseUrl: () => "http://server",
			eventSourceFactory: (url) => new FakeEventSource(url),
		});
		const states: PlaybackState[] = [];
		const queueEvents: unknown[] = [];
		const errors: Error[] = [];
		const detachState = stream.addListener(
			"playback-state",
			(event) => states.push(parsePlaybackState(event.data)),
			(error) => errors.push(error),
		);
		await Promise.resolve();
		await Promise.resolve();

		expect(FakeEventSource.instances).toHaveLength(1);
		const source = FakeEventSource.instances[0];
		const detachQueue = stream.addListener(
			"queue-invalidated",
			(event) => queueEvents.push(JSON.parse(event.data)),
			(error) => errors.push(error),
		);
		await Promise.resolve();
		expect(FakeEventSource.instances).toHaveLength(1);

		source.emit("playback-state", JSON.stringify(state));
		source.emit("queue-invalidated", JSON.stringify({ revision: "2" }));
		expect(states).toEqual([state]);
		expect(queueEvents).toEqual([{ revision: "2" }]);

		const lateStates: PlaybackState[] = [];
		const detachLate = stream.addListener(
			"playback-state",
			(event) => lateStates.push(parsePlaybackState(event.data)),
			(error) => errors.push(error),
		);
		source.emit("playback-state", JSON.stringify(state));
		expect(lateStates).toEqual([state]);

		detachState();
		detachLate();
		detachQueue();
		expect(source.closed).toBe(true);
		expect(errors).toHaveLength(0);

		const detachReopen = stream.addListener(
			"playback-state",
			(event) => states.push(parsePlaybackState(event.data)),
			(error) => errors.push(error),
		);
		await Promise.resolve();
		await Promise.resolve();
		expect(FakeEventSource.instances).toHaveLength(2);
		detachReopen();
	});

	it("dispatches once per frame after a kind leaves and comes back", async () => {
		const stream = createSharedQueueEventStream({
			getBaseUrl: () => "http://server",
			eventSourceFactory: (url) => new FakeEventSource(url),
		});
		const errors = () => undefined;
		const detachQueue = stream.addListener(
			"queue-invalidated",
			() => undefined,
			errors,
		);
		const detachState = stream.addListener(
			"playback-state",
			() => undefined,
			errors,
		);
		await Promise.resolve();
		await Promise.resolve();
		detachState();
		const delivered: string[] = [];
		const detachAgain = stream.addListener(
			"playback-state",
			(event) => delivered.push(event.data),
			errors,
		);

		FakeEventSource.instances[0].emit("playback-state", "{}");

		expect(delivered).toEqual(["{}"]);
		detachAgain();
		detachQueue();
	});

	it("reports a failed base URL to every listener", async () => {
		const stream = createSharedQueueEventStream({
			getBaseUrl: () => Promise.reject(new Error("offline")),
			eventSourceFactory: (url) => new FakeEventSource(url),
		});
		const errors: Error[] = [];
		const detachA = stream.addListener(
			"playback-state",
			() => undefined,
			(error) => errors.push(error),
		);
		const detachB = stream.addListener(
			"queue-invalidated",
			() => undefined,
			(error) => errors.push(error),
		);
		await Promise.resolve();
		await Promise.resolve();

		expect(errors).toHaveLength(2);
		detachA();
		detachB();
	});
});
