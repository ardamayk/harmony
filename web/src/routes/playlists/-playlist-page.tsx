import type { QueueItemSource } from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { Search } from "lucide-react";
import { CollectionDetailHeader } from "#/components/collection-detail-header";
import { DetailPageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { Input } from "#/components/ui/input";
import { apiClient } from "#/lib/api";
import {
	invalidatePlaylistCache,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";
import {
	formatTrackCollectionDuration,
	useTrackCollectionViewState,
} from "#/lib/track-collection-view-state";

// Route components live outside the route file: with automatic code
// splitting, a component defined inside the route module can be rendered
// from its split chunk before that chunk finished evaluating after an HMR
// update in WebKit, which surfaces as "_s is not a function".
export function PlaylistDetailPage() {
	const { playlistId } = useParams({ from: "/playlists/$playlistId" });
	return <PlaylistDetailContent playlistId={playlistId} />;
}

export function PlaylistDetailContent({ playlistId }: { playlistId: string }) {
	const queryClient = useQueryClient();
	const navigate = useNavigate();
	const { playTrack, queueTracks } = usePlayback();
	const playlist = useQuery({
		queryKey: playlistQueryKeys.detail(playlistId),
		queryFn: () => apiClient.getPlaylist(playlistId),
	});

	const removeTrack = useMutation({
		mutationFn: (trackId: string) =>
			apiClient.removePlaylistTrack(playlistId, trackId),
		onSuccess: async (nextPlaylist) => {
			await invalidatePlaylistCache(queryClient, playlistId);
			if (!nextPlaylist.isDefault && nextPlaylist.tracks.length === 0) {
				await navigate({ to: "/playlists" });
			}
		},
	});
	const source: QueueItemSource | undefined = playlist.data
		? {
				kind: "playlist",
				playlistId: playlist.data.id,
				name: playlist.data.name,
			}
		: undefined;
	const collection = useTrackCollectionViewState(
		playlist.data?.tracks ?? [],
		{
			playTrack,
			queueTracks,
		},
		source,
	);

	if (playlist.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading playlist…</div>;
	}

	if (playlist.isError || !playlist.data) {
		return (
			<div className="p-6 text-destructive text-sm">Playlist not found</div>
		);
	}

	const data = playlist.data;
	const trackCount = data.trackCount ?? data.tracks.length;

	return (
		<DetailPageShell testId="playlist-detail-content">
			<CollectionDetailHeader
				kind="Playlist"
				title={data.name}
				subtitle={data.isDefault ? "Default playlist" : "User playlist"}
				metaTags={[
					`${trackCount} track${trackCount === 1 ? "" : "s"}`,
					collection.totalDurationMs > 0
						? formatTrackCollectionDuration(collection.totalDurationMs)
						: "Duration unknown",
				]}
				trackCount={collection.visibleTracks.length}
				coverTracks={data.tracks}
				onPlay={collection.handlePlay}
				onShuffle={collection.handleShuffle}
				onQueue={collection.handleQueue}
			/>

			<section className="mt-6">
				{data.tracks.length > 0 ? (
					<div
						data-testid="playlist-track-search"
						className="relative mb-4 w-full"
					>
						<Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-caption" />
						<Input
							className="h-11 rounded-lg bg-player pl-8 text-sm"
							placeholder={`Search ${data.name}…`}
							value={collection.search}
							onChange={(event) => collection.setSearch(event.target.value)}
						/>
					</div>
				) : null}
				{data.tracks.length === 0 ? (
					<p className="text-foreground text-sm">This playlist is empty.</p>
				) : collection.visibleTracks.length === 0 ? (
					<p className="text-foreground text-sm">
						No tracks match this search.
					</p>
				) : (
					<TrackList
						tracks={collection.visibleTracks}
						contextTracks={collection.visibleTracks}
						source={source}
						playMode="double"
						numbering="list"
						showFavorite
						showMeta
						compact
						onRemoveTrack={(track) => removeTrack.mutate(track.id)}
						onDeleteTrackSuccess={() => {
							if (!data.isDefault && data.tracks.length === 1) {
								void navigate({ to: "/playlists" });
							}
						}}
						removeLabel="Remove from playlist"
					/>
				)}
			</section>
		</DetailPageShell>
	);
}
