import type { QueryClient } from "@tanstack/react-query";
import { libraryQueryKeys } from "#/lib/library-query-keys";

export const PLAYLIST_PREVIEW_STALE_TIME_MS = 60_000;

export const playlistQueryKeys = {
	list: ["playlists"] as const,
	detail: (playlistId: string) => ["playlist", playlistId] as const,
	favoritesFallback: ["playlist", "favorites"] as const,
	preview: (playlistId: string) => ["playlist", playlistId, "preview"] as const,
};

export async function invalidatePlaylistCache(
	queryClient: QueryClient,
	playlistId?: string,
) {
	const invalidations = [
		queryClient.invalidateQueries({ queryKey: libraryQueryKeys.searches }),
		queryClient.invalidateQueries({ queryKey: playlistQueryKeys.list }),
	];
	if (playlistId) {
		invalidations.push(
			queryClient.invalidateQueries({
				queryKey: playlistQueryKeys.detail(playlistId),
			}),
			queryClient.invalidateQueries({
				queryKey: playlistQueryKeys.preview(playlistId),
			}),
		);
	} else {
		invalidations.push(
			queryClient.invalidateQueries({
				queryKey: ["playlist"],
			}),
		);
	}
	await Promise.all(invalidations);
}
