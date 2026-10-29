import type { PlaybackApi } from "@repo/ui";
import type { QueryClient } from "@tanstack/react-query";
import { apiClient } from "#/lib/api";
import { externalPlaybackQueryOptions } from "#/lib/external-playback-query";

/**
 * The Music Server API for playback. External Track resolves read through the
 * query cache, so one the search dialog prefetched is reused on play.
 */
export function createPlaybackApi(queryClient: QueryClient): PlaybackApi {
	return {
		...playbackApi,
		getExternalTrackPlayback: (externalTrackId) =>
			queryClient.fetchQuery(externalPlaybackQueryOptions(externalTrackId)),
	};
}

const playbackApi: Omit<PlaybackApi, "getExternalTrackPlayback"> = {
	getQueue: () => apiClient.getPlaybackQueue(),
	replaceQueue: (trackIds, revision, source) =>
		apiClient.replacePlaybackQueue(trackIds, revision, source),
	reorderQueue: (itemIds, revision) =>
		apiClient.reorderPlaybackQueue(itemIds, revision),
	appendQueueItem: (trackId, revision, source) =>
		apiClient.appendPlaybackQueueItem(trackId, revision, source),
	removeQueueItem: (itemId, revision) =>
		apiClient.removePlaybackQueueItem(itemId, revision),
	clearQueue: (afterItemId, revision) =>
		apiClient.clearPlaybackQueue(afterItemId, revision),
	subscribeQueueEvents: (onEvent, onError) =>
		apiClient.subscribePlaybackQueueEvents(onEvent, onError),
	getStreamUrl: (trackId) => apiClient.getTrackStreamUrl(trackId),
	headTrackStream: (trackId) => apiClient.headTrackStream(trackId),
	getAlbumCoverUrl: (albumId, size) =>
		apiClient.getAlbumCoverUrl(albumId, size),
	getTrack: (trackId) => apiClient.getTrack(trackId),
	getTrackLyrics: (trackId) => apiClient.getTrackLyrics(trackId),
	getTrackWaveform: (trackId) => apiClient.getTrackWaveform(trackId),
	getRadioStationStreamUrl: (stationId) =>
		apiClient.getRadioStationStreamUrl(stationId),
	getRadioCatalogPreviewStreamUrl: (stationUuid) =>
		apiClient.getRadioCatalogPreviewStreamUrl(stationUuid),
	getRadioNowPlaying: (stationId) => apiClient.getRadioNowPlaying(stationId),
	getExternalStreamUrl: (streamPath) =>
		apiClient.getExternalStreamUrl(streamPath),
	getExternalTrackArtworkUrl: (externalTrackId) =>
		apiClient.getExternalTrackArtworkUrl(externalTrackId),
	listPlaylists: () => apiClient.listPlaylists(),
	getPlaylist: (playlistId) => apiClient.getPlaylist(playlistId),
	createPlaylist: (name) => apiClient.createPlaylist({ name }),
	addPlaylistTrack: (playlistId, trackId) =>
		apiClient.addPlaylistTrack(playlistId, trackId),
	removePlaylistTrack: (playlistId, trackId) =>
		apiClient.removePlaylistTrack(playlistId, trackId),
};
