import type { Playlist } from "@repo/api-client";
import { AlbumArt, NAV_LINK_CLASS } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { apiClient } from "#/lib/api";
import {
	PLAYLIST_PREVIEW_STALE_TIME_MS,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";
import { cn } from "#/lib/utils";

/** One Playlist, set in under the Sidebar's link, with its first Track's cover. */
function SidebarPlaylist({ playlist }: { playlist: Playlist }) {
	const preview = useQuery({
		queryKey: playlistQueryKeys.preview(playlist.id),
		queryFn: () => apiClient.getPlaylist(playlist.id),
		enabled: playlist.trackCount > 0,
		staleTime: PLAYLIST_PREVIEW_STALE_TIME_MS,
	});
	const firstTrack = preview.data?.tracks[0];
	return (
		<Link
			to="/playlists/$playlistId"
			params={{ playlistId: playlist.id }}
			title={playlist.name}
			className={cn(NAV_LINK_CLASS, "ml-4 h-9 px-2")}
		>
			<AlbumArt
				coverUrl={
					firstTrack
						? apiClient.getAlbumCoverUrl(firstTrack.albumId, 64)
						: undefined
				}
				title={playlist.name}
				className="size-7 shrink-0 rounded-sm text-xs"
			/>
			<span className="truncate">{playlist.name}</span>
		</Link>
	);
}

/** The Playlists one by one under their Sidebar link, like Spotify's. */
export function SidebarPlaylists() {
	const playlists = useQuery({
		queryKey: playlistQueryKeys.list,
		queryFn: () => apiClient.listPlaylists(),
	});
	const items = playlists.data?.items ?? [];
	if (items.length === 0) return null;
	return (
		<nav aria-label="Playlists">
			<ul className="flex flex-col gap-1">
				{items.map((playlist) => (
					<li key={playlist.id}>
						<SidebarPlaylist playlist={playlist} />
					</li>
				))}
			</ul>
		</nav>
	);
}
