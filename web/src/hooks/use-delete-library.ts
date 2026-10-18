import type { AlbumDeletionResult } from "@repo/api-client";
import { toast, usePlayback } from "@repo/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useLocation, useNavigate } from "@tanstack/react-router";
import { apiClient } from "#/lib/api";
import { invalidateLibraryCache } from "#/lib/invalidate-library-cache";
import { invalidatePlaylistCache } from "#/lib/playlist-query-cache";

async function syncPlaybackAfterDelete(
	playback: ReturnType<typeof usePlayback>,
	trackId?: string,
) {
	await playback.refreshQueue();

	if (trackId && playback.currentTrack?.id === trackId) {
		await playback.clearQueue();
	}
}

export function useDeleteTrack() {
	const queryClient = useQueryClient();
	const playback = usePlayback();

	return useMutation({
		mutationFn: ({
			trackId,
			confirmationToken,
		}: {
			trackId: string;
			confirmationToken: string;
		}) => apiClient.deleteTrack(trackId, confirmationToken),
		onSuccess: async (_result, { trackId }) => {
			// Cache first, then playback: sync must observe the post-deletion queue.
			await invalidateLibraryCache(queryClient, { trackId });
			await Promise.all([
				invalidatePlaylistCache(queryClient),
				syncPlaybackAfterDelete(playback, trackId),
			]);
		},
	});
}

export function usePreviewTrackDeletion() {
	return useMutation({
		mutationFn: (trackId: string) => apiClient.previewTrackDeletion(trackId),
	});
}

export function usePreviewAlbumDeletion() {
	return useMutation({
		mutationFn: (albumId: string) => apiClient.previewAlbumDeletion(albumId),
	});
}

function describeAlbumDeletion(result: AlbumDeletionResult): string {
	const deleted = `${result.deleted.length} track${result.deleted.length === 1 ? "" : "s"} deleted`;
	if (!result.stoppedAt) return deleted;
	return `${deleted}; stopped at "${result.stoppedAt.trackTitle}": ${result.stoppedAt.reason}`;
}

/**
 * Deletes an Album as one Permanent Track Deletion per Track. The result is
 * per Track, so a partial run is reported as such rather than as an error:
 * the tracks already gone stay gone.
 */
export function useDeleteAlbum() {
	const queryClient = useQueryClient();
	const navigate = useNavigate();
	const location = useLocation();
	const playback = usePlayback();

	return useMutation({
		mutationFn: ({
			albumId,
			confirmationToken,
		}: {
			albumId: string;
			confirmationToken: string;
		}) => apiClient.deleteAlbum(albumId, confirmationToken),
		onSuccess: async (result, { albumId }) => {
			const deletedIds = new Set(result.deleted.map((track) => track.trackId));
			const albumGone = result.stoppedAt === null;
			const firstDeleted = result.deleted[0];
			await invalidateLibraryCache(
				queryClient,
				albumGone
					? { albumId }
					: firstDeleted
						? { trackId: firstDeleted.trackId }
						: {},
			);
			await invalidatePlaylistCache(queryClient);
			if (playback.currentTrack && deletedIds.has(playback.currentTrack.id)) {
				await playback.clearQueue();
			} else {
				await playback.refreshQueue();
			}
			if (albumGone) {
				toast.success("All tracks deleted", {
					description: describeAlbumDeletion(result),
				});
				if (location.pathname.includes(albumId)) {
					void navigate({ to: "/library/albums" });
				}
			} else {
				toast.error("Deletion stopped", {
					description: describeAlbumDeletion(result),
				});
			}
		},
		onError: (cause) => {
			toast.error("Tracks could not be deleted", {
				description: cause.message,
			});
		},
	});
}
