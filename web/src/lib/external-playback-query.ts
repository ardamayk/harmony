import { queryOptions } from "@tanstack/react-query";
import { apiClient } from "#/lib/api";

/**
 * Well under the Music Server's own resolve cache (up to 10 minutes, less
 * when the media URL expires sooner), so a reused resolve still streams.
 */
export const EXTERNAL_PLAYBACK_STALE_MS = 2 * 60 * 1000;

/**
 * An External Track's playback resolution. The search dialog prefetches it
 * and the PlaybackApi reads through it, so a prefetched resolve is reused
 * when the track is played.
 */
export function externalPlaybackQueryOptions(externalTrackId: string) {
	return queryOptions({
		queryKey: ["external-playback", externalTrackId] as const,
		queryFn: ({ signal }) =>
			apiClient.getExternalTrackPlayback(externalTrackId, signal),
		staleTime: EXTERNAL_PLAYBACK_STALE_MS,
		retry: false,
	});
}
