import type { Album, QueueItemSource, Track } from "@repo/api-client";
import { getAlbumArtistName } from "#/lib/library-display";

type QueueTracks = (
	trackIds: string[],
	source?: QueueItemSource,
) => Promise<void>;

export async function queueTracksByAlbum(
	tracks: Track[],
	queueTracks: QueueTracks,
	getAlbum: (albumId: string) => Promise<Album>,
): Promise<void> {
	const sources = new Map<string, QueueItemSource>();
	let startIndex = 0;
	while (startIndex < tracks.length) {
		const albumId = tracks[startIndex].albumId;
		let endIndex = startIndex + 1;
		while (endIndex < tracks.length && tracks[endIndex].albumId === albumId) {
			endIndex += 1;
		}
		let source = sources.get(albumId);
		if (!source) {
			const album = await getAlbum(albumId);
			source = {
				kind: "album",
				albumId,
				albumTitle: album.title,
				artistName: getAlbumArtistName(album),
			};
			sources.set(albumId, source);
		}
		await queueTracks(
			tracks.slice(startIndex, endIndex).map((track) => track.id),
			source,
		);
		startIndex = endIndex;
	}
}
