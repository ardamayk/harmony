import {
	hasServerCapability,
	SHARED_SESSION_CAPABILITY,
} from "@repo/api-client";
import { type PlaybackEngine, SharedPlaybackEngine } from "@repo/ui";
import { isDesktopClient } from "#/desktop/bridge";
import { createDesktopSharedPlaybackTransport } from "#/desktop/shared-playback-transport";
import { apiClient } from "#/lib/api";
import { createBrowserSharedPlaybackTransport } from "#/lib/shared-playback-transport";
import { BrowserPlaybackEngine } from "./BrowserPlaybackEngine";
import { DesktopPlaybackEngine } from "./DesktopPlaybackEngine";

async function isSharedPlaybackSupported() {
	return hasServerCapability(
		await apiClient.getHealth(),
		SHARED_SESSION_CAPABILITY,
	);
}

export function createPlaybackEngine(): PlaybackEngine {
	return isDesktopClient()
		? new SharedPlaybackEngine(
				new DesktopPlaybackEngine(),
				createDesktopSharedPlaybackTransport(),
				isSharedPlaybackSupported,
			)
		: new SharedPlaybackEngine(
				new BrowserPlaybackEngine(),
				createBrowserSharedPlaybackTransport(),
				isSharedPlaybackSupported,
			);
}
