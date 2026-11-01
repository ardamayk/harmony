import type { components } from "./generated/schema";
import type { SharedQueueEventStream } from "./queue-events";

/**
 * The Music Server advertises the Shared Playback State and its
 * `playback-state` frames on the Queue event stream with this capability
 * (ADR 0040). Without it clients behave as before.
 */
export const SHARED_SESSION_CAPABILITY = "playback.shared-session.v1";

export type PlaybackState = components["schemas"]["PlaybackState"];
export type PlaybackStateCommand =
	components["schemas"]["PlaybackStateCommand"];
export type PlaybackStateConflictResponse =
	components["schemas"]["PlaybackStateConflictResponse"];
export type PlaybackSkipRequest = components["schemas"]["PlaybackSkipRequest"];

/**
 * The shape of a failed request the api-client raises; structural, so this
 * module stays free of the client's own error class.
 */
type PlaybackRequestFailure = {
	status: number;
	body: { code: string } & { state?: PlaybackState };
};

/**
 * Reports whether a failed command answer means the sender is no longer the
 * Active Device, and returns the state inside it: the current truth the
 * sender must accept instead of its own.
 */
export function playbackConflictState(
	error: unknown,
): PlaybackState | undefined {
	const failure = error as PlaybackRequestFailure | undefined;
	if (
		failure &&
		typeof failure === "object" &&
		failure.status === 409 &&
		typeof failure.body === "object" &&
		failure.body !== null &&
		failure.body.code === "playback_device_not_active" &&
		failure.body.state
	) {
		return failure.body.state;
	}
	return undefined;
}

/**
 * Validates one `playback-state` frame's payload: a full snapshot with the
 * values the Stream contract expects. Frames arrive as plain JSON strings.
 */
export function parsePlaybackState(data: string): PlaybackState {
	const state: unknown = JSON.parse(data);
	if (
		typeof state !== "object" ||
		state === null ||
		!("revision" in state) ||
		typeof state.revision !== "number" ||
		!("playing" in state) ||
		typeof state.playing !== "boolean" ||
		!("positionMs" in state) ||
		typeof state.positionMs !== "number" ||
		!("positionAtMs" in state) ||
		typeof state.positionAtMs !== "number" ||
		!("activeDeviceId" in state) ||
		!("activeDeviceName" in state) ||
		!("remoteCommandRevision" in state) ||
		typeof state.remoteCommandRevision !== "number" ||
		!("serverTimeMs" in state) ||
		typeof state.serverTimeMs !== "number"
	) {
		throw new TypeError("Playback state event has an invalid payload");
	}
	return state as PlaybackState;
}

/**
 * Delivers every Shared Playback State snapshot over the `playback-state`
 * frames of the Queue event stream: one on connecting and one per change.
 * The stream is the client's one EventSource, shared with the Queue events.
 */
export function subscribePlaybackState(
	stream: SharedQueueEventStream,
	onState: (state: PlaybackState) => void,
	onError: (error: Error) => void = (error) => {
		console.warn("Shared playback state stream error", { error });
	},
): () => void {
	return stream.addListener(
		"playback-state",
		(event) => {
			try {
				onState(parsePlaybackState(event.data));
			} catch (error) {
				onError(error instanceof Error ? error : new Error(String(error)));
			}
		},
		onError,
	);
}
