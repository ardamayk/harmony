import type { QueueItemSource } from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { AlbumDetailHeader } from "#/components/album-detail-header";
import { MoreFromArtist } from "#/components/more-from-artist";
import { DetailPageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { useTrackPlayCounts } from "#/hooks/use-track-play-counts";
import { apiClient } from "#/lib/api";
import { getAlbumArtistName } from "#/lib/library-display";
import { isGone } from "#/lib/query-persistence";

// Route components live outside the route file: with automatic code
// splitting, a component defined inside the route module can be rendered
// from its split chunk before that chunk finished evaluating after an HMR
// update in WebKit, which surfaces as "_s is not a function".
export function AlbumDetailPage() {
	const { albumId } = useParams({ from: "/library/$albumId" });
	return <AlbumDetailContent albumId={albumId} />;
}

export function AlbumDetailContent({ albumId }: { albumId: string }) {
	const { playTrack, queueTracks } = usePlayback();
	const album = useQuery({
		queryKey: ["library", "album", albumId],
		queryFn: () => apiClient.getAlbum(albumId),
		staleTime: 0,
	});
	const playCounts = useTrackPlayCounts(album.data?.tracks ?? []);

	if (album.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading album…</div>;
	}

	if (!album.data || isGone(album.error)) {
		return <div className="p-6 text-destructive text-sm">Album not found</div>;
	}

	const data = album.data;
	const source: QueueItemSource = {
		kind: "album",
		albumId: data.id,
		albumTitle: data.title,
		artistName: getAlbumArtistName(data),
	};

	const handlePlayAlbum = () => {
		const first = data.tracks[0];
		if (!first) return;
		void playTrack(
			first.id,
			data.tracks.map((track) => track.id),
			source,
		);
	};

	const handleQueueAlbum = () => {
		void queueTracks(
			data.tracks.map((track) => track.id),
			source,
		);
	};

	return (
		<DetailPageShell testId="album-detail-content">
			<AlbumDetailHeader
				album={data}
				onPlayAlbum={handlePlayAlbum}
				onQueueAlbum={handleQueueAlbum}
			/>

			<section className="mt-6">
				<TrackList
					tracks={data.tracks}
					albumId={data.id}
					source={source}
					showFavorite
					showMeta
					compact
					phoneDownloads
					playCounts={playCounts}
				/>
			</section>

			<MoreFromArtist
				artistId={data.artistId}
				artistName={getAlbumArtistName(data)}
				excludeAlbumId={data.id}
			/>
		</DetailPageShell>
	);
}
