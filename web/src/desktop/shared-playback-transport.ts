import type { PlaybackState } from "@repo/api-client";
import { DESKTOP_EVENTS } from "@repo/desktop-bridge";
import type { SharedPlaybackTransport } from "@repo/ui";
import { apiClient } from "#/lib/api";
import {
	resolveSharedPlaybackSource,
	type SharedPlaybackSourceApi,
} from "#/lib/shared-playback-transport";
import { invoke, listen } from "./desktop-api";

/**
 * The Desktop's `SharedPlaybackTransport`. Every server contact goes through
 * the Rust core, so Background Playback keeps one device identity and one
 * reporter with the window closed. Creating it and subscribing sends nothing:
 * only `sendCommand`, from a user's play, reaches the server.
 */
export function createDesktopSharedPlaybackTransport(
	api: SharedPlaybackSourceApi = apiClient,
): SharedPlaybackTransport {
	return {
		getDevice: () => invoke("get_shared_playback_device"),
		async getState() {
			const state = await invoke<PlaybackState | null>(
				"get_shared_playback_state",
			);
			// SharedPlaybackEngine stays on the local Player and retries.
			if (!state) {
				throw new Error("The Desktop core has no Shared Playback State.");
			}
			return state;
		},
		sendCommand: (command) =>
			invoke("send_shared_playback_command", { command }),
		subscribe: (onState) => listen(DESKTOP_EVENTS.sharedPlaybackState, onState),
		resolveSource: (state) => resolveSharedPlaybackSource(state, api),
	};
}
