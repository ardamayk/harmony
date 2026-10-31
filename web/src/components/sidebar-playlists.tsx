import { NAV_LINK_CLASS } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { apiClient } from "#/lib/api";
import { playlistQueryKeys } from "#/lib/playlist-query-cache";
import { cn } from "#/lib/utils";

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
						<Link
							to="/playlists/$playlistId"
							params={{ playlistId: playlist.id }}
							title={playlist.name}
							className={cn(NAV_LINK_CLASS, "h-8")}
						>
							<span className="truncate">{playlist.name}</span>
						</Link>
					</li>
				))}
			</ul>
		</nav>
	);
}
