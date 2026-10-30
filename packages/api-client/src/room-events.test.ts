import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { ApiError, createApiClient, type RoomEvent } from "./index";
import type { QueueEventSource } from "./queue-events";

const room = {
	revision: 3,
	trackIds: ["00000000-0000-4000-8000-000000000001"],
	currentIndex: 0,
	playing: true,
	positionMs: 1000,
	positionAtMs: 2000,
	lyricsOpen: false,
	guestCount: 1,
	maxGuests: 8,
	expiresAt: "2026-10-31T12:00:00Z",
};

class FakeEventSource implements QueueEventSource {
	static instances: FakeEventSource[] = [];
	listeners = new Map<string, (event: MessageEvent<string>) => void>();
	closed = false;
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

function notFound() {
	return jsonResponse({ error: "x", code: "not_found", message: "gone" }, 404);
}

function createClient(transport: typeof fetch, mode?: "stream" | "poll") {
	return createApiClient({
		baseUrl: "http://server",
		transport,
		roomEventsMode: mode,
		eventSourceFactory: (url) => new FakeEventSource(url),
		validateResponses: false,
	});
}

describe("Listen Together client", () => {
	it("sends a command and returns the Room State", async () => {
		const transport = vi.fn(async () => jsonResponse(room)) as never;
		const client = createClient(transport);

		const result = await client.sendListenTogetherCommand({
			type: "seek",
			positionMs: 5000,
		});

		expect(result.revision).toBe(3);
		const [url, init] = (transport as unknown as ReturnType<typeof vi.fn>).mock
			.calls[0];
		expect(url).toBe("http://server/api/v1/listen-together/room/commands");
		expect(init.method).toBe("POST");
		expect(JSON.parse(init.body)).toEqual({ type: "seek", positionMs: 5000 });
	});

	it("posts the Invite Link token to join and rejects with the API error", async () => {
		const client = createClient(
			vi.fn(async () =>
				jsonResponse(
					{ error: "x", code: "room_full", message: "The Room is full" },
					409,
				),
			) as never,
		);

		const failure = await client
			.joinListenTogetherRoom("token")
			.catch((e) => e);

		expect(failure).toBeInstanceOf(ApiError);
		expect(failure.body.code).toBe("room_full");
	});
});

describe("Room event stream", () => {
	beforeEach(() => {
		FakeEventSource.instances = [];
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("delivers snapshots and stops at room-closed", () => {
		const events: RoomEvent[] = [];
		const client = createClient(vi.fn() as never);

		client.subscribeListenTogetherRoomEvents((event) => events.push(event));
		const source = FakeEventSource.instances[0];
		expect(source.url).toBe("http://server/api/v1/listen-together/room/events");
		source.emit("room-state", JSON.stringify(room));
		source.emit("room-closed");

		expect(events).toEqual([
			{ type: "room-state", room },
			{ type: "room-closed" },
		]);
		expect(source.closed).toBe(true);
	});

	it("reports a malformed snapshot without dropping the stream", () => {
		const onError = vi.fn();
		const client = createClient(vi.fn() as never);

		client.subscribeListenTogetherRoomEvents(vi.fn(), onError);
		FakeEventSource.instances[0].emit("room-state", "{}");

		expect(onError).toHaveBeenCalledOnce();
		expect(FakeEventSource.instances[0].closed).toBe(false);
	});

	it("reads the Room State and reconnects after a dropped stream", async () => {
		const events: RoomEvent[] = [];
		const client = createClient(vi.fn(async () => jsonResponse(room)) as never);

		client.subscribeListenTogetherRoomEvents(
			(event) => events.push(event),
			vi.fn(),
		);
		FakeEventSource.instances[0].onerror?.(new Event("error"));
		await vi.advanceTimersByTimeAsync(1000);

		expect(events).toEqual([{ type: "room-state", room }]);
		expect(FakeEventSource.instances).toHaveLength(2);
		expect(FakeEventSource.instances[0].closed).toBe(true);
	});

	it("turns a refused reconnect into room-closed", async () => {
		const events: RoomEvent[] = [];
		const client = createClient(vi.fn(async () => notFound()) as never);

		client.subscribeListenTogetherRoomEvents(
			(event) => events.push(event),
			vi.fn(),
		);
		FakeEventSource.instances[0].onerror?.(new Event("error"));
		await vi.advanceTimersByTimeAsync(1000);

		expect(events).toEqual([{ type: "room-closed" }]);
		expect(FakeEventSource.instances).toHaveLength(1);
	});

	it("stops reconnecting once unsubscribed", async () => {
		const transport = vi.fn(async () => jsonResponse(room)) as never;
		const client = createClient(transport);

		const unsubscribe = client.subscribeListenTogetherRoomEvents(
			vi.fn(),
			vi.fn(),
		);
		unsubscribe();
		await vi.advanceTimersByTimeAsync(5000);

		expect(FakeEventSource.instances[0].closed).toBe(true);
		expect(FakeEventSource.instances).toHaveLength(1);
	});
});

describe("Room event polling", () => {
	beforeEach(() => {
		FakeEventSource.instances = [];
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("delivers each revision once without opening a stream", async () => {
		const events: RoomEvent[] = [];
		const transport = vi
			.fn()
			.mockImplementationOnce(async () => jsonResponse(room))
			.mockImplementationOnce(async () => jsonResponse(room))
			.mockImplementation(async () =>
				jsonResponse({ ...room, revision: 4 }),
			) as never;
		const client = createClient(transport, "poll");

		const unsubscribe = client.subscribeListenTogetherRoomEvents((event) =>
			events.push(event),
		);
		await vi.advanceTimersByTimeAsync(1600);
		unsubscribe();

		expect(
			events.map((event) => event.type === "room-state" && event.room.revision),
		).toEqual([3, 4]);
		expect(FakeEventSource.instances).toHaveLength(0);
	});

	it("reports room-closed when the Room is gone", async () => {
		const events: RoomEvent[] = [];
		const client = createClient(vi.fn(async () => notFound()) as never, "poll");

		client.subscribeListenTogetherRoomEvents((event) => events.push(event));
		await vi.advanceTimersByTimeAsync(0);

		expect(events).toEqual([{ type: "room-closed" }]);
	});

	it("polls by default when requests carry a token", async () => {
		const client = createApiClient({
			baseUrl: "http://server",
			getToken: () => "token",
			transport: vi.fn(async () => notFound()) as never,
			eventSourceFactory: (url) => new FakeEventSource(url),
			validateResponses: false,
		});

		client.subscribeListenTogetherRoomEvents(vi.fn());
		await vi.advanceTimersByTimeAsync(0);

		expect(FakeEventSource.instances).toHaveLength(0);
	});
});
