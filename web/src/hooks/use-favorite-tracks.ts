import { useHiddenFromGuest } from "@repo/ui/guest-access";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import { apiClient } from "#/lib/api";
import {
	invalidatePlaylistCache,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";

export function useFavoriteTracks() {
	const queryClient = useQueryClient();
	const hideFavorites = useHiddenFromGuest("favorites");
	const playlists = useQuery({
		queryKey: playlistQueryKeys.list,
		queryFn: () => apiClient.listPlaylists(),
		enabled: !hideFavorites,
	});

	const favoritesPlaylist = playlists.data?.items.find(
		(playlist) => playlist.isDefault && playlist.name === "Favorites",
	);

	const favoritesDetail = useQuery({
		queryKey: favoritesPlaylist
			? playlistQueryKeys.detail(favoritesPlaylist.id)
			: playlistQueryKeys.favoritesFallback,
		queryFn: () => apiClient.getPlaylist(favoritesPlaylist?.id ?? ""),
		enabled: Boolean(favoritesPlaylist),
	});

	const favorites = useMemo(
		() => favoritesDetail.data?.tracks.map((track) => track.id) ?? [],
		[favoritesDetail.data?.tracks],
	);

	const addFavorite = useMutation({
		mutationFn: ({
			playlistId,
			trackId,
		}: {
			playlistId: string;
			trackId: string;
		}) => apiClient.addPlaylistTrack(playlistId, trackId),
		onSuccess: async (_data, vars) => {
			await invalidatePlaylistCache(queryClient, vars.playlistId);
		},
	});

	const removeFavorite = useMutation({
		mutationFn: ({
			playlistId,
			trackId,
		}: {
			playlistId: string;
			trackId: string;
		}) => apiClient.removePlaylistTrack(playlistId, trackId),
		onSuccess: async (_data, vars) => {
			await invalidatePlaylistCache(queryClient, vars.playlistId);
		},
	});

	const isFavorite = useCallback(
		(trackId: string) => favorites.includes(trackId),
		[favorites],
	);

	const toggleFavorite = useCallback(
		(trackId: string) => {
			if (!favoritesPlaylist) return;
			const vars = { playlistId: favoritesPlaylist.id, trackId };
			if (favorites.includes(trackId)) {
				removeFavorite.mutate(vars);
			} else {
				addFavorite.mutate(vars);
			}
		},
		[addFavorite, favorites, favoritesPlaylist, removeFavorite],
	);

	return { favorites, isFavorite, toggleFavorite };
}
