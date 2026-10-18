import type { AlbumDetail, AlbumList } from "@repo/api-client";
import type { QueryClient } from "@tanstack/react-query";
import { libraryQueryKeys } from "#/lib/library-query-keys";

type InvalidateOptions = {
	albumId?: string;
	trackId?: string;
};

export async function invalidateLibraryCache(
	queryClient: QueryClient,
	options: InvalidateOptions = {},
) {
	const { albumId, trackId } = options;

	if (albumId) {
		queryClient.removeQueries({ queryKey: libraryQueryKeys.album(albumId) });

		queryClient.setQueriesData<AlbumList>(
			{ queryKey: libraryQueryKeys.root },
			(current) => {
				if (
					!current ||
					!("items" in current) ||
					!Array.isArray(current.items)
				) {
					return current;
				}
				const items = current.items.filter(
					(item) => !("id" in item) || item.id !== albumId,
				);
				if (items.length === current.items.length) return current;
				return { ...current, items, total: Math.max(0, current.total - 1) };
			},
		);
	}

	if (trackId) {
		queryClient.setQueriesData<AlbumDetail>(
			{ queryKey: ["library", "album"] },
			(current) => {
				if (!current?.tracks) return current;
				const tracks = current.tracks.filter((track) => track.id !== trackId);
				if (tracks.length === current.tracks.length) return current;
				return {
					...current,
					tracks,
					trackCount: tracks.length,
				};
			},
		);
	}

	await queryClient.invalidateQueries({
		queryKey: libraryQueryKeys.root,
	});
	// Inactive search entries only go stale; refetching every cached query
	// string after each mutation fans out into many full-library rankings.
	await queryClient.invalidateQueries({
		queryKey: libraryQueryKeys.searches,
		refetchType: "none",
	});
}
