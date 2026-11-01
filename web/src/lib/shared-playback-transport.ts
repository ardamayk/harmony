import {
	ApiError,
	type CoverSize,
	type PlaybackState,
	type Track,
} from "@repo/api-client";
import type { PlaybackSource, SharedPlaybackTransport } from "@repo/ui";
import { apiClient } from "#/lib/api";

const DEVICE_ID_KEY = "harmony.shared-playback-device-id";
const DEVICE_ID_CHANNEL = "harmony.shared-playback-device-id";
const DEVICE_ID_ANSWER_MS = 250;

type DeviceIdMessage = { id: string } & (
	| { claim: string }
	| { refused: string }
);

/** What `resolveSharedPlaybackSource` needs from the Music Server API. */
export type SharedPlaybackSourceApi = {
	getTrack(trackId: string): Promise<Track>;
	getTrackStreamUrl(trackId: string): string;
	getAlbumCoverUrl(albumId: string, size: CoverSize): string;
};

/**
 * The source a Queue item plays, for the Track a snapshot names. Null for a
 * non-library source, an empty state, or a Track that no longer exists.
 */
export async function resolveSharedPlaybackSource(
	state: PlaybackState,
	api: SharedPlaybackSourceApi,
): Promise<PlaybackSource | null> {
	const trackId = state.trackId;
	if (!trackId) return null;
	let track: Track;
	try {
		track = await api.getTrack(trackId);
	} catch (error) {
		// Only a 404 says the Track is gone; the engine retries anything else.
		if (error instanceof ApiError && error.status === 404) return null;
		throw error;
	}
	return {
		type: "track",
		track,
		playbackUrl: api.getTrackStreamUrl(trackId),
		queueItemId: state.queueItemId ?? undefined,
		artworkUrl: api.getAlbumCoverUrl(track.albumId, 512),
	};
}

let pageDeviceId: Promise<string> | null = null;

/**
 * Two tabs are two Players, and a reload keeps its ID (ADR 0040). Duplicating
 * a tab copies `sessionStorage`, so a stored ID counts only once the open tabs
 * have been asked: a tab holding it refuses, while a reloaded page's old self
 * is gone. Of two tabs opening with one ID, the lower random claim keeps it.
 * A tab only ever moves to a new ID, so tabs cannot swap theirs.
 *
 * shortcut: a tab too busy or frozen to answer in time shares its ID until the
 * duplicate reloads; an ID that can change after the handshake would fix that.
 */
export function claimBrowserDeviceId(
	storage: Pick<Storage, "getItem" | "setItem"> = sessionStorage,
) {
	const takeNewId = () => {
		const id = randomId();
		storage.setItem(DEVICE_ID_KEY, id);
		return id;
	};
	const stored = storage.getItem(DEVICE_ID_KEY);
	let deviceId = stored ?? takeNewId();
	const claim = randomId();
	const channel = new BroadcastChannel(DEVICE_ID_CHANNEL);
	return new Promise<string>((resolve) => {
		let waiting = stored !== null;
		const settle = () => {
			waiting = false;
			resolve(deviceId);
		};
		const moveAside = () => {
			deviceId = takeNewId();
			settle();
		};
		channel.onmessage = ({ data }: MessageEvent<DeviceIdMessage>) => {
			if (data.id !== deviceId) return;
			if ("refused" in data) {
				if (waiting && data.refused === claim) moveAside();
			} else if (waiting && data.claim < claim) {
				moveAside();
			} else {
				channel.postMessage({ id: deviceId, refused: data.claim });
			}
		};
		if (!waiting) {
			settle();
			return;
		}
		channel.postMessage({ id: deviceId, claim });
		setTimeout(settle, DEVICE_ID_ANSWER_MS);
	});
}

/** `randomUUID` exists only on HTTPS, and a Music Server is often plain HTTP. */
function randomId() {
	if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
	return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
		byte.toString(16).padStart(2, "0"),
	).join("");
}

/** Such as "Firefox on Linux". */
export function browserDeviceName(userAgent: string) {
	const browser =
		[
			["Edg/", "Edge"],
			["OPR/", "Opera"],
			["Firefox/", "Firefox"],
			["Chrome/", "Chrome"],
			["Safari/", "Safari"],
		].find(([token]) => userAgent.includes(token as string))?.[1] ?? "Browser";
	const system =
		[
			["Windows", "Windows"],
			["Android", "Android"],
			["iPhone", "iOS"],
			["iPad", "iOS"],
			["Mac OS", "macOS"],
			["Linux", "Linux"],
		].find(([token]) => userAgent.includes(token as string))?.[1] ?? null;
	return system ? `${browser} on ${system}` : browser;
}

export function createBrowserSharedPlaybackTransport(): SharedPlaybackTransport {
	return {
		getDevice: async () => ({
			id: await (pageDeviceId ??= claimBrowserDeviceId()),
			name: browserDeviceName(navigator.userAgent),
		}),
		getState: () => apiClient.getPlaybackState(),
		sendCommand: (command) => apiClient.sendPlaybackStateCommand(command),
		subscribe: (onState) => apiClient.subscribePlaybackState(onState),
		resolveSource: (state) => resolveSharedPlaybackSource(state, apiClient),
	};
}
