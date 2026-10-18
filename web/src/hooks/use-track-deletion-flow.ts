import type { Track, TrackDeletionPreview } from "@repo/api-client";
import { useDeleteTrack, usePreviewTrackDeletion } from "./use-delete-library";
import { useDeletionFlow } from "./use-deletion-flow";

export function useTrackDeletionFlow(onDeleted?: (track: Track) => void) {
	const deleteTrack = useDeleteTrack();
	const flow = useDeletionFlow<Track, TrackDeletionPreview>({
		preview: usePreviewTrackDeletion(),
		remove: {
			isPending: deleteTrack.isPending,
			mutate: ({ id, confirmationToken }, callbacks) =>
				deleteTrack.mutate({ trackId: id, confirmationToken }, callbacks),
		},
		onDeleted,
	});
	return { ...flow, track: flow.subject };
}
