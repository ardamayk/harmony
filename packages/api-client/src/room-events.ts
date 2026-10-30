import type { components } from "./generated/schema";
import type { QueueEventSource } from "./queue-events";

const ROOM_EVENTS_PATH = "/api/v1/listen-together/room/events";
const RECONNECT_INITIAL_DELAY_MS = 1000;
const RECONNECT_MAX_DELAY_MS = 5000;
/** How often a client without a stream reads the Room State. */
export const ROOM_POLL_INTERVAL_MS = 500;

type ListenTogetherRoom = components["schemas"]["ListenTogetherRoom"];

export type RoomEvent =
	| { type: "room-state"; room: ListenTogetherRoom }
	| { type: "room-closed" };

/**
 * "stream" is the server-sent events the contract defines. "poll" reads the
 * Room State instead, for a client whose requests cannot reach the stream.
 */
export type RoomEventsMode = "stream" | "poll";

type RoomEventSubscription = {
	mode: RoomEventsMode;
	getBaseUrl: () => string;
	getRoom: () => Promise<ListenTogetherRoom>;
	/** Whether a failed Room read means the Room or this session is over. */
	isRoomGone: (error: unknown) => boolean;
	eventSourceFactory: (url: string) => QueueEventSource;
};

/**
 * Delivers every snapshot and, once, the end of the Room. A dropped stream
 * reconnects after reading the Room State, so a Room that ended meanwhile
 * still reports room-closed: EventSource hides the status of a refused
 * reconnect.
 */
export function subscribeRoomEvents(
	config: RoomEventSubscription,
	onEvent: (event: RoomEvent) => void,
	onError?: (error: Error) => void,
): () => void {
	const reportError =
		onError ??
		((error: Error) => {
			console.warn("Room event stream error", { error });
		});
	const abortController = new AbortController();
	const run = config.mode === "poll" ? pollRoom : streamRoom;
	void run(config, onEvent, reportError, abortController.signal);
	return () => abortController.abort();
}

async function streamRoom(
	config: RoomEventSubscription,
	onEvent: (event: RoomEvent) => void,
	reportError: (error: Error) => void,
	signal: AbortSignal,
): Promise<void> {
	let delayMs = RECONNECT_INITIAL_DELAY_MS;
	while (!signal.aborted) {
		const outcome = await streamUntilDisconnected(
			config,
			onEvent,
			reportError,
			signal,
		);
		if (outcome === "closed" || signal.aborted) return;
		reportError(new Error("Room event stream disconnected"));
		await waitForDelay(signal, delayMs);
		if (signal.aborted) return;
		try {
			const room = await config.getRoom();
			if (signal.aborted) return;
			onEvent({ type: "room-state", room });
			delayMs = RECONNECT_INITIAL_DELAY_MS;
		} catch (error) {
			if (signal.aborted) return;
			if (config.isRoomGone(error)) {
				onEvent({ type: "room-closed" });
				return;
			}
			reportError(toError(error));
			delayMs = Math.min(delayMs * 2, RECONNECT_MAX_DELAY_MS);
		}
	}
}

function streamUntilDisconnected(
	config: RoomEventSubscription,
	onEvent: (event: RoomEvent) => void,
	reportError: (error: Error) => void,
	signal: AbortSignal,
): Promise<"closed" | "disconnected"> {
	return new Promise((resolve) => {
		const eventSource = config.eventSourceFactory(buildRoomEventsUrl(config));
		const finish = (outcome: "closed" | "disconnected") => {
			eventSource.close();
			signal.removeEventListener("abort", handleAbort);
			resolve(outcome);
		};
		const handleAbort = () => finish("disconnected");
		signal.addEventListener("abort", handleAbort, { once: true });
		eventSource.addEventListener("room-state", (event) => {
			try {
				onEvent({ type: "room-state", room: parseRoom(event.data) });
			} catch (error) {
				reportError(toError(error));
			}
		});
		eventSource.addEventListener("room-closed", () => {
			finish("closed");
			onEvent({ type: "room-closed" });
		});
		eventSource.onerror = () => finish("disconnected");
	});
}

async function pollRoom(
	config: RoomEventSubscription,
	onEvent: (event: RoomEvent) => void,
	reportError: (error: Error) => void,
	signal: AbortSignal,
): Promise<void> {
	let revision: number | undefined;
	while (!signal.aborted) {
		try {
			const room = await config.getRoom();
			if (signal.aborted) return;
			if (room.revision !== revision) {
				revision = room.revision;
				onEvent({ type: "room-state", room });
			}
		} catch (error) {
			if (signal.aborted) return;
			if (config.isRoomGone(error)) {
				onEvent({ type: "room-closed" });
				return;
			}
			reportError(toError(error));
		}
		await waitForDelay(signal, ROOM_POLL_INTERVAL_MS);
	}
}

function parseRoom(data: string): ListenTogetherRoom {
	const room: unknown = JSON.parse(data);
	if (
		typeof room !== "object" ||
		room === null ||
		!("revision" in room) ||
		typeof room.revision !== "number" ||
		!("trackIds" in room) ||
		!Array.isArray(room.trackIds)
	) {
		throw new TypeError("Room event has an invalid payload");
	}
	return room as ListenTogetherRoom;
}

function buildRoomEventsUrl(config: RoomEventSubscription): string {
	return `${config.getBaseUrl().replace(/\/$/, "")}${ROOM_EVENTS_PATH}`;
}

function waitForDelay(signal: AbortSignal, delayMs: number): Promise<void> {
	return new Promise((resolve) => {
		if (signal.aborted) {
			resolve();
			return;
		}
		const handleAbort = () => {
			clearTimeout(timeoutId);
			resolve();
		};
		const timeoutId = setTimeout(() => {
			signal.removeEventListener("abort", handleAbort);
			resolve();
		}, delayMs);
		signal.addEventListener("abort", handleAbort, { once: true });
	});
}

function toError(error: unknown): Error {
	return error instanceof Error ? error : new Error(String(error));
}
