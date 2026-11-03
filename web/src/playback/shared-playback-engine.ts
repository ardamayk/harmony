import type { PlaybackEngine } from "@repo/ui";
import { createPlaybackEngine } from "./create-playback-engine";

type PlaybackEngineFactory = () => PlaybackEngine;

let sharedPlaybackEngine: PlaybackEngine | null = null;

export function getSharedPlaybackEngine(
	createEngine: PlaybackEngineFactory = createPlaybackEngine,
): PlaybackEngine {
	if (!sharedPlaybackEngine) {
		sharedPlaybackEngine = createEngine();
	}
	return sharedPlaybackEngine;
}

/**
 * Stops what is playing, then disposes the engine. Disposing alone leaves
 * the Desktop core's mpv playing, since it keeps background playback.
 */
export function stopSharedPlaybackEngine(): void {
	sharedPlaybackEngine?.stop();
	disposeSharedPlaybackEngine();
}

export function disposeSharedPlaybackEngine(): void {
	sharedPlaybackEngine?.destroy();
	sharedPlaybackEngine = null;
}

if (import.meta.hot) {
	import.meta.hot.dispose(disposeSharedPlaybackEngine);
}
