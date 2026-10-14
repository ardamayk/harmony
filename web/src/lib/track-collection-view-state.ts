import type { QueueItemSource, Track } from "@repo/api-client";
import { toast } from "@repo/ui";
import { useMemo, useState } from "react";
import { apiClient } from "#/lib/api";
import { filterTracksByText } from "#/lib/filter-tracks";
import { queueTracksByAlbum } from "#/lib/queue-tracks-by-album";

type TrackCollectionPlayback = {
	playTrack: (
		trackId: string,
		queueTrackIds?: string[],
		source?: QueueItemSource,
	) => Promise<void>;
	queueTracks: (trackIds: string[], source?: QueueItemSource) => Promise<void>;
};

export function formatTrackCollectionDuration(ms: number): string {
	if (!ms || ms < 0) return "0m";
	const total = Math.floor(ms / 1000);
	const hours = Math.floor(total / 3600);
	const minutes = Math.floor((total % 3600) / 60);
	if (hours > 0) return `${hours}h ${minutes}m`;
	return `${minutes}m`;
}

function shuffleTrackCollection(tracks: Track[]): Track[] {
	const next = [...tracks];
	for (let i = next.length - 1; i > 0; i -= 1) {
		const j = Math.floor(Math.random() * (i + 1));
		[next[i], next[j]] = [next[j], next[i]];
	}
	return next;
}

export function useTrackCollectionViewState(
	tracks: Track[],
	playback: TrackCollectionPlayback,
	source?: QueueItemSource,
) {
	const [search, setSearch] = useState("");
	const visibleTracks = useMemo(
		() => filterTracksByText(tracks, search),
		[tracks, search],
	);
	const visibleTrackIds = useMemo(
		() => visibleTracks.map((track) => track.id),
		[visibleTracks],
	);
	const totalDurationMs = useMemo(
		() => tracks.reduce((sum, track) => sum + (track.durationMs ?? 0), 0),
		[tracks],
	);

	const handlePlay = () => {
		const first = visibleTracks[0];
		if (!first) return;
		void playback.playTrack(
			first.id,
			visibleTrackIds,
			...(source ? [source] : []),
		);
	};

	const handleShuffle = () => {
		const shuffled = shuffleTrackCollection(visibleTracks);
		const first = shuffled[0];
		if (!first) return;
		void playback.playTrack(
			first.id,
			shuffled.map((track) => track.id),
			...(source ? [source] : []),
		);
	};

	const handleQueue = () => {
		void queueTracksByAlbum(visibleTracks, playback.queueTracks, (albumId) =>
			apiClient.getAlbum(albumId),
		).catch((error) => {
			console.warn("Failed to queue collection tracks", {
				trackIds: visibleTrackIds,
				error,
			});
			toast.error("Failed to queue collection tracks");
		});
	};

	return {
		search,
		setSearch,
		visibleTracks,
		visibleTrackIds,
		totalDurationMs,
		handlePlay,
		handleShuffle,
		handleQueue,
	};
}
