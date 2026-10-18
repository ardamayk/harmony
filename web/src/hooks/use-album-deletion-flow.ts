import type { Album, AlbumDeletionPreview } from "@repo/api-client";
import { useDeleteAlbum, usePreviewAlbumDeletion } from "./use-delete-library";
import { useDeletionFlow } from "./use-deletion-flow";

type AlbumSummary = Pick<Album, "id" | "title">;

export function useAlbumDeletionFlow(
	onDeleted?: (album: AlbumSummary) => void,
) {
	const deleteAlbum = useDeleteAlbum();
	const flow = useDeletionFlow<AlbumSummary, AlbumDeletionPreview>({
		preview: usePreviewAlbumDeletion(),
		remove: {
			isPending: deleteAlbum.isPending,
			mutate: ({ id, confirmationToken }, callbacks) =>
				deleteAlbum.mutate({ albumId: id, confirmationToken }, callbacks),
		},
		onDeleted,
	});
	return { ...flow, album: flow.subject };
}
