import {
	ApiError,
	type CoverSize,
	type PlaybackState,
	type Track,
} from "@repo/api-client";
import type { PlaybackSource, SharedPlaybackTransport } from "@repo/ui";
import { apiClient } from "#/lib/api";

const DEVICE_ID_KEY = "harmony.shared-playback-device-id";

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

/** Two tabs are two Players, and a reload keeps its ID (ADR 0040). */
function browserDeviceId() {
	const stored = sessionStorage.getItem(DEVICE_ID_KEY);
	if (stored) return stored;
	const id = randomId();
	sessionStorage.setItem(DEVICE_ID_KEY, id);
	return id;
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
			id: browserDeviceId(),
			name: browserDeviceName(navigator.userAgent),
		}),
		getState: () => apiClient.getPlaybackState(),
		sendCommand: (command) => apiClient.sendPlaybackStateCommand(command),
		subscribe: (onState) => apiClient.subscribePlaybackState(onState),
		resolveSource: (state) => resolveSharedPlaybackSource(state, apiClient),
	};
}
