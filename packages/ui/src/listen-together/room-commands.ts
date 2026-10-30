import type {
	ListenTogetherCommand,
	ListenTogetherRoom,
} from "@repo/api-client";

/** The contract's limit on one playTracks command. */
const MAX_ROOM_QUEUE_TRACKS = 500;

/**
 * The command for playing a library Track in a Room. With a context it
 * becomes the Room Queue; alone, the Track plays next in the Room Queue, or
 * from its place if it is already there.
 */
export function playTrackCommand(
	room: ListenTogetherRoom,
	trackId: string,
	queueTrackIds?: string[],
): ListenTogetherCommand {
	if (queueTrackIds) return playContext(trackId, queueTrackIds);
	const queuedIndex = room.trackIds.indexOf(trackId);
	if (queuedIndex >= 0) {
		return { type: "playTracks", trackIds: room.trackIds, index: queuedIndex };
	}
	const insertAt = (room.currentIndex ?? -1) + 1;
	const trackIds = [...room.trackIds];
	trackIds.splice(insertAt, 0, trackId);
	return playContext(trackId, trackIds, insertAt);
}

function playContext(
	trackId: string,
	trackIds: string[],
	knownIndex?: number,
): ListenTogetherCommand {
	const index = knownIndex ?? trackIds.indexOf(trackId);
	if (index < 0) {
		return playContext(trackId, [trackId, ...trackIds], 0);
	}
	if (trackIds.length <= MAX_ROOM_QUEUE_TRACKS) {
		return { type: "playTracks", trackIds, index };
	}
	// Longer than a Room Queue holds: keep what follows the Track.
	return {
		type: "playTracks",
		trackIds: trackIds.slice(index, index + MAX_ROOM_QUEUE_TRACKS),
		index: 0,
	};
}
