import { useEffect, useState, useSyncExternalStore } from "react";

export type TrackWaveformResult =
	| { trackId: string; peakCount: number; peaks: number[] }
	| { status: "pending"; retryAfterSeconds: number };

export type TrackWaveformLoader = (
	trackId: string,
) => Promise<TrackWaveformResult>;

const waveformCache = new Map<string, number[]>();
const cacheListeners = new Set<() => void>();
let cacheRevision = 0;
const MAX_PENDING_POLLS = 30;

function subscribeCache(listener: () => void) {
	cacheListeners.add(listener);
	return () => {
		cacheListeners.delete(listener);
	};
}

function getCacheRevision() {
	return cacheRevision;
}

/** Forget cached waveforms after a library change and reload mounted tracks. */
export function clearTrackWaveformCache() {
	waveformCache.clear();
	cacheRevision += 1;
	for (const listener of cacheListeners) listener();
}

/**
 * Peaks for the current Track, or null while unknown. The server generates
 * peaks lazily and answers "pending" meanwhile, so the hook polls at the
 * server's suggested interval. Cached peaks display immediately, then revalidate
 * on mount or invalidation so replacing a Track's file refreshes its waveform.
 */
export function useTrackWaveform({
	trackId,
	enabled,
	load,
}: {
	trackId: string | null;
	enabled: boolean;
	load: TrackWaveformLoader | null;
}): number[] | null {
	// Peaks the latest request returned, tagged with what it was loaded for.
	const [loaded, setLoaded] = useState<{
		trackId: string;
		revision: number;
		peaks: number[] | null;
	} | null>(null);
	const revision = useSyncExternalStore(
		subscribeCache,
		getCacheRevision,
		getCacheRevision,
	);

	useEffect(() => {
		if (!enabled || !trackId || !load) return undefined;
		let cancelled = false;
		let timer: number | null = null;
		let polls = 0;
		const request = () => {
			if (cancelled || revision !== cacheRevision) return;
			load(trackId)
				.then((result) => {
					if (cancelled || revision !== cacheRevision) return;
					if ("status" in result) {
						waveformCache.delete(trackId);
						setLoaded({ trackId, revision, peaks: null });
						polls += 1;
						if (polls > MAX_PENDING_POLLS) return;
						timer = window.setTimeout(
							request,
							Math.max(1, result.retryAfterSeconds) * 1000,
						);
						return;
					}
					waveformCache.set(trackId, result.peaks);
					setLoaded({ trackId, revision, peaks: result.peaks });
				})
				.catch(() => {
					// No waveform (older server, missing ffmpeg, missing file): the
					// seek bar simply stays plain.
				});
		};
		request();
		return () => {
			cancelled = true;
			if (timer !== null) window.clearTimeout(timer);
		};
	}, [enabled, trackId, load, revision]);

	if (!enabled || !trackId || !load) return null;
	if (loaded?.trackId === trackId && loaded.revision === revision) {
		return loaded.peaks;
	}
	return waveformCache.get(trackId) ?? null;
}
