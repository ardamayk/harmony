import type { components } from "./generated/schema";

const QUEUE_EVENT_RECONNECT_DELAY_MS = 1000;
const EVENT_SOURCE_CLOSED = 2;
const CAPABILITY_RETRY_INITIAL_DELAY_MS = 1000;
const CAPABILITY_RETRY_MAX_DELAY_MS = 30000;
const QUEUE_EVENTS_PATH = "/api/v1/playback/queue/events";
export const QUEUE_EVENTS_CAPABILITY = "playback.queue-events.v1";

export type QueueEvent = components["schemas"]["QueueEvent"];

export type QueueEventSource = {
	addEventListener: (
		name: string,
		listener: (event: MessageEvent<string>) => void,
	) => void;
	close: () => void;
	onerror: ((event: Event) => void) | null;
	readyState?: number;
};

type QueueEventSubscription = {
	getBaseUrl: () => string | Promise<string>;
	getCapabilities: () => Promise<string[]>;
	getToken?: () => string | undefined;
	transport: typeof fetch;
	eventSourceFactory: (url: string) => QueueEventSource;
	subscriber?: (
		onEvent: (event: QueueEvent) => void,
		onError: (error: Error) => void,
	) => (() => void) | Promise<() => void>;
	/**
	 * One EventSource per client, shared with the Shared Playback State
	 * subscription (ADR 0040). Without it the Queue events open their own.
	 */
	sharedStream?: SharedQueueEventStream;
};

type SharedStreamListener = (event: MessageEvent<string>) => void;

/**
 * One EventSource per client for both event kinds the Queue event stream
 * carries: `queue-invalidated` and, when the Shared Playback State is
 * advertised (ADR 0040), `playback-state`. The connection opens with the
 * first listener and closes with the last, so a client holds one stream.
 */
export type SharedQueueEventStream = {
	/** Attaches one named-event listener; the returned call removes it. */
	addListener: (
		eventName: string,
		listener: SharedStreamListener,
		onError: (error: Error) => void,
	) => () => void;
};

export function createSharedQueueEventStream(config: {
	getBaseUrl: () => string | Promise<string>;
	eventSourceFactory: (url: string) => QueueEventSource;
}): SharedQueueEventStream {
	const listeners = new Map<string, Set<SharedStreamListener>>();
	const errorHandlers = new Set<(error: Error) => void>();
	let source: QueueEventSource | null = null;
	/** Event names `source` already dispatches; each once per connection. */
	const attached = new Set<string>();
	let opening = false;
	let closed = false;

	function dispatch(eventName: string, event: MessageEvent<string>) {
		for (const listener of listeners.get(eventName) ?? []) listener(event);
	}

	function attach(eventName: string) {
		if (!source || attached.has(eventName)) return;
		attached.add(eventName);
		source.addEventListener(eventName, (event) => dispatch(eventName, event));
	}

	function open() {
		if (source || opening || closed) return;
		opening = true;
		void Promise.resolve(config.getBaseUrl())
			.then((baseUrl) => {
				opening = false;
				if (closed) return;
				source = config.eventSourceFactory(buildQueueEventsUrl(baseUrl));
				for (const eventName of listeners.keys()) attach(eventName);
				source.onerror = () => {
					// EventSource reconnects on its own; say why the frames stop.
					const error = new Error("Queue event stream disconnected");
					for (const handler of errorHandlers) handler(error);
					// A non-200 answer, such as a proxy's 502 during a restart, ends
					// EventSource for good instead.
					if (source?.readyState === EVENT_SOURCE_CLOSED) {
						source = null;
						attached.clear();
						setTimeout(open, QUEUE_EVENT_RECONNECT_DELAY_MS);
					}
				};
			})
			.catch((error) => {
				opening = false;
				for (const handler of errorHandlers) handler(toError(error));
			});
	}

	return {
		addListener(eventName, listener, onError) {
			closed = false;
			let set = listeners.get(eventName);
			if (!set) {
				set = new Set();
				listeners.set(eventName, set);
			}
			set.add(listener);
			errorHandlers.add(onError);
			attach(eventName);
			open();
			return () => {
				set.delete(listener);
				errorHandlers.delete(onError);
				const hasListeners = [...listeners.values()].some(
					(entries) => entries.size > 0,
				);
				if (!hasListeners) {
					closed = true;
					source?.close();
					source = null;
					attached.clear();
				}
			};
		},
	};
}

export function subscribeQueueEvents(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	onError?: (error: Error) => void,
): () => void {
	const reportError =
		onError ??
		((error: Error) => {
			console.warn("Queue event stream error", { error });
		});
	const abortController = new AbortController();
	let unsubscribe: (() => void) | undefined;
	void subscribeAfterCapabilityCheck(
		config,
		onEvent,
		reportError,
		abortController.signal,
	)
		.then((cleanup) => {
			if (!cleanup) return;
			if (abortController.signal.aborted) cleanup();
			else unsubscribe = cleanup;
		})
		.catch((error) => {
			if (!abortController.signal.aborted) reportError(toError(error));
		});
	return () => {
		abortController.abort();
		unsubscribe?.();
	};
}

async function subscribeAfterCapabilityCheck(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
	signal: AbortSignal,
): Promise<(() => void) | undefined> {
	let retryDelayMs = CAPABILITY_RETRY_INITIAL_DELAY_MS;
	while (!signal.aborted) {
		let capabilities: string[];
		try {
			capabilities = await config.getCapabilities();
		} catch (error) {
			if (signal.aborted) return undefined;
			reportError(toError(error));
			await waitForDelay(signal, retryDelayMs);
			retryDelayMs = Math.min(retryDelayMs * 2, CAPABILITY_RETRY_MAX_DELAY_MS);
			continue;
		}
		if (signal.aborted) return undefined;
		if (!capabilities.includes(QUEUE_EVENTS_CAPABILITY)) {
			reportMissingCapability(reportError);
			return undefined;
		}
		return subscribeSupportedQueueEvents(config, onEvent, reportError);
	}
	return undefined;
}

function reportMissingCapability(reportError: (error: Error) => void): void {
	reportError(
		new Error(
			`Music Server does not advertise ${QUEUE_EVENTS_CAPABILITY}; Queue synchronization is disabled.`,
		),
	);
}

function subscribeSupportedQueueEvents(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
): () => void {
	if (config.subscriber) {
		return subscribeWithCustomSubscriber(
			config.subscriber,
			onEvent,
			reportError,
		);
	}
	if (config.getToken) {
		return subscribeWithFetch(config, onEvent, reportError);
	}
	if (config.sharedStream) {
		return config.sharedStream.addListener(
			"queue-invalidated",
			(event) => {
				try {
					onEvent(parseQueueEvent(event.data));
				} catch (error) {
					reportError(toError(error));
				}
			},
			reportError,
		);
	}
	return subscribeWithEventSource(config, onEvent, reportError);
}

function subscribeWithCustomSubscriber(
	subscriber: NonNullable<QueueEventSubscription["subscriber"]>,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
): () => void {
	let unsubscribe: (() => void) | undefined;
	let isClosed = false;
	void Promise.resolve(subscriber(onEvent, reportError))
		.then((cleanup) => {
			if (isClosed) cleanup();
			else unsubscribe = cleanup;
		})
		.catch((error) => reportError(toError(error)));
	return () => {
		isClosed = true;
		unsubscribe?.();
	};
}

function subscribeWithEventSource(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
): () => void {
	let eventSource: QueueEventSource | undefined;
	let isClosed = false;
	void Promise.resolve(config.getBaseUrl())
		.then((baseUrl) => {
			if (isClosed) return;
			eventSource = config.eventSourceFactory(buildQueueEventsUrl(baseUrl));
			eventSource.addEventListener("queue-invalidated", (event) => {
				try {
					onEvent(parseQueueEvent(event.data));
				} catch (error) {
					reportError(toError(error));
				}
			});
			eventSource.onerror = () => {
				reportError(new Error("Queue event stream disconnected"));
			};
		})
		.catch((error) => reportError(toError(error)));
	return () => {
		isClosed = true;
		eventSource?.close();
	};
}

function subscribeWithFetch(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
): () => void {
	const abortController = new AbortController();
	void runFetchLoop(config, onEvent, reportError, abortController.signal);
	return () => abortController.abort();
}

async function runFetchLoop(
	config: QueueEventSubscription,
	onEvent: (event: QueueEvent) => void,
	reportError: (error: Error) => void,
	signal: AbortSignal,
): Promise<void> {
	let lastEventId: string | undefined;
	while (!signal.aborted) {
		try {
			const headers = new Headers({ Accept: "text/event-stream" });
			const token = config.getToken?.();
			if (token) headers.set("Authorization", `Bearer ${token}`);
			if (lastEventId) headers.set("Last-Event-ID", lastEventId);
			const response = await config.transport(
				buildQueueEventsUrl(await config.getBaseUrl()),
				{
					headers,
					signal,
				},
			);
			lastEventId = await readQueueEventStream(response, onEvent, lastEventId);
		} catch (error) {
			if (signal.aborted) return;
			reportError(toError(error));
		}
		await waitForDelay(signal, QUEUE_EVENT_RECONNECT_DELAY_MS);
	}
}

async function readQueueEventStream(
	response: Response,
	onEvent: (event: QueueEvent) => void,
	lastEventId?: string,
): Promise<string | undefined> {
	if (!response.ok || !response.body) {
		throw new Error(`Queue event stream returned HTTP ${response.status}`);
	}
	const reader = response.body.getReader();
	const decoder = new TextDecoder();
	let buffer = "";
	while (true) {
		const { done, value } = await reader.read();
		if (done) return lastEventId;
		buffer += decoder.decode(value, { stream: true }).replaceAll("\r\n", "\n");
		let frameEnd = buffer.indexOf("\n\n");
		while (frameEnd >= 0) {
			const frame = buffer.slice(0, frameEnd);
			buffer = buffer.slice(frameEnd + 2);
			const decoded = decodeQueueEventFrame(frame);
			if (decoded) {
				lastEventId = decoded.id;
				onEvent(decoded.event);
			}
			frameEnd = buffer.indexOf("\n\n");
		}
	}
}

function decodeQueueEventFrame(
	frame: string,
): { id: string; event: QueueEvent } | undefined {
	let id = "";
	let name = "";
	const data: string[] = [];
	for (const line of frame.split("\n")) {
		if (line.startsWith("id: ")) id = line.slice(4);
		else if (line.startsWith("event: ")) name = line.slice(7);
		else if (line.startsWith("data: ")) data.push(line.slice(6));
	}
	if (!id || name !== "queue-invalidated") return undefined;
	return { id, event: parseQueueEvent(data.join("\n")) };
}

function parseQueueEvent(data: string): QueueEvent {
	const event: unknown = JSON.parse(data);
	if (
		typeof event !== "object" ||
		event === null ||
		!("revision" in event) ||
		typeof event.revision !== "string" ||
		!("sequence" in event) ||
		typeof event.sequence !== "string" ||
		!/^\d+$/.test(event.sequence) ||
		!("invalidates" in event) ||
		!Array.isArray(event.invalidates) ||
		!event.invalidates.every((value) => value === "queue")
	) {
		throw new TypeError("Queue event has an invalid payload");
	}
	return event as QueueEvent;
}

function buildQueueEventsUrl(baseUrl: string): string {
	return `${baseUrl.replace(/\/$/, "")}${QUEUE_EVENTS_PATH}`;
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
