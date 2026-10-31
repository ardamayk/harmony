import type { Playlist } from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ListMusic, TriangleAlert } from "lucide-react";
import {
	CARD_OVERLAY_CLASS,
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionCardArt,
	CollectionGrid,
	CollectionGridSkeleton,
	CollectionGridState,
	CollectionPageContainer,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";
import {
	PLAYLIST_PREVIEW_STALE_TIME_MS,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";
import { cn } from "#/lib/utils";

export function PlaylistsPage() {
	const playlists = useQuery({
		queryKey: playlistQueryKeys.list,
		queryFn: () => apiClient.listPlaylists(),
	});
	const playlistItems = playlists.data?.items ?? [];
	const isInitialLoading = playlists.isLoading && !playlists.data;
	const hasError = playlists.isError && !playlists.data;

	return (
		<PageShell
			testId="playlists-page-shell"
			contentTestId="playlists-page-content"
			header={
				<PageHeader
					title="Playlists"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
				/>
			}
		>
			<CollectionPageContainer aria-busy={playlists.isFetching || undefined}>
				{isInitialLoading ? (
					<CollectionGridSkeleton label="Loading playlists" />
				) : hasError ? (
					<CollectionGridState
						kind="error"
						icon={<TriangleAlert aria-hidden />}
						title="Unable to load playlists"
						description="Check your connection and try again."
						onRetry={() => void playlists.refetch()}
						isRetrying={playlists.isFetching}
					/>
				) : playlistItems.length === 0 ? (
					<CollectionGridState
						kind="empty"
						icon={<ListMusic aria-hidden />}
						title="No playlists yet"
						description="Create a playlist to organize your library."
					/>
				) : (
					<CollectionGrid isBusy={playlists.isFetching}>
						{playlistItems.map((playlist) => (
							<PlaylistCard key={playlist.id} playlist={playlist} />
						))}
					</CollectionGrid>
				)}
			</CollectionPageContainer>
		</PageShell>
	);
}

function PlaylistCard({ playlist }: { playlist: Playlist }) {
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
			className={cn(
				"group relative aspect-square overflow-hidden rounded-md border border-border bg-card",
				CARD_INTERACTION_CLASS,
			)}
		>
			{firstTrack ? (
				<CollectionCardArt albumId={firstTrack.albumId} title={playlist.name} />
			) : null}
			<div data-playlist-card-overlay className={CARD_OVERLAY_CLASS}>
				<p className="truncate font-semibold text-heading text-xs leading-tight">
					{playlist.name}
				</p>
				<p className="truncate text-xs text-caption leading-tight">
					{playlist.trackCount} track{playlist.trackCount === 1 ? "" : "s"}
				</p>
			</div>
		</Link>
	);
}
