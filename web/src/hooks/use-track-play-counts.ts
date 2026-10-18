import type { Track } from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { apiClient } from "#/lib/api";

/**
 * Last.fm play counts for these Tracks, by Track id. Empty when the Music
 * Server has no Last.fm key (ADR 0030); it refreshes them weekly, so an hour
 * here is plenty.
 */
export function useTrackPlayCounts(tracks: Track[]): Map<string, number> {
	const ids = tracks.map((track) => track.id);
	const counts = useQuery({
		queryKey: ["library", "play-counts", ids.join(",")],
		queryFn: () => apiClient.listTrackPlayCounts(ids),
		enabled: ids.length > 0,
		staleTime: 60 * 60_000,
	});
	return counts.data ?? EMPTY;
}

const EMPTY = new Map<string, number>();
