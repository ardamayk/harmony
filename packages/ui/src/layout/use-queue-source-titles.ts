import { ApiError, type QueueItem } from "@repo/api-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { usePlayback } from "../playback/PlaybackProvider";

/** Source tracks can remain in the library after leaving the queue. */
export function useQueueSourceTitles(queue: QueueItem[]) {
	const { getTrack } = usePlayback();
	const [titles, setTitles] = useState<Record<string, string | null>>({});
	const retryableIdsRef = useRef(new Set<string>());
	const retryFailedLookups = useCallback(() => {
		const trackIds = [...retryableIdsRef.current];
		if (trackIds.length === 0) return;
		retryableIdsRef.current.clear();
		setTitles((current) => {
			const next = { ...current };
			for (const trackId of trackIds) delete next[trackId];
			return next;
		});
	}, []);
	useEffect(() => {
		// A fresh queue or restored connection gives transient failures one new attempt.
		if (queue.length > 0) retryFailedLookups();
		window.addEventListener("online", retryFailedLookups);
		return () => window.removeEventListener("online", retryFailedLookups);
	}, [queue, retryFailedLookups]);
	useEffect(() => {
		const seedIds = new Set(
			queue.flatMap((item) =>
				item.source?.kind === "suggestion" ? item.source.basedOn : [],
			),
		);
		const missingIds = [...seedIds].filter(
			(trackId) =>
				!(trackId in titles) &&
				!queue.some((item) => item.track.id === trackId),
		);
		if (missingIds.length === 0) return;
		let isActive = true;
		void Promise.all(
			missingIds.map(async (trackId) => {
				try {
					const track = await getTrack(trackId);
					return [trackId, track?.title ?? null] as const;
				} catch (error) {
					if (isActive && !(error instanceof ApiError && error.status === 404))
						retryableIdsRef.current.add(trackId);
					console.warn("Failed to load queue suggestion source track", {
						trackId,
						error,
					});
					return [trackId, null] as const;
				}
			}),
		).then((entries) => {
			if (isActive)
				setTitles((current) => ({
					...current,
					...Object.fromEntries(entries),
				}));
		});
		return () => {
			isActive = false;
		};
	}, [getTrack, queue, titles]);
	return titles;
}
