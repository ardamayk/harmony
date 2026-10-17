import type { Album, AlbumDeletionPreview } from "@repo/api-client";
import {
	DeletionActions,
	DeletionRows,
	formatDeletionBytes,
} from "#/components/track-deletion-dialog";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "#/components/ui/dialog";

export function AlbumDeletionDialog({
	album,
	preview,
	error,
	isLoading,
	isDeleting,
	onCancel,
	onConfirm,
}: {
	album: Pick<Album, "id" | "title"> | null;
	preview: AlbumDeletionPreview | null;
	error: string | null;
	isLoading: boolean;
	isDeleting: boolean;
	onCancel: () => void;
	onConfirm: () => void;
}) {
	return (
		<Dialog open={Boolean(album)} onOpenChange={(open) => !open && onCancel()}>
			<DialogContent>
				<DialogTitle>
					Permanently delete every track of {album?.title}?
				</DialogTitle>
				<DialogDescription className="mt-2">
					Each track and its managed file is deleted one by one. This cannot be
					undone; if a track fails, the ones already deleted stay deleted.
				</DialogDescription>
				{isLoading ? (
					<p className="mt-4 text-sm">Loading deletion details…</p>
				) : null}
				{preview ? <AlbumDeletionPreviewDetails preview={preview} /> : null}
				{error ? (
					<p role="alert" className="mt-4 text-destructive text-sm">
						{error}
					</p>
				) : null}
				<DeletionActions
					isReady={Boolean(preview)}
					isDeleting={isDeleting}
					onCancel={onCancel}
					onConfirm={onConfirm}
				/>
			</DialogContent>
		</Dialog>
	);
}

function AlbumDeletionPreviewDetails({
	preview,
}: {
	preview: AlbumDeletionPreview;
}) {
	const queueCount = preview.queueReferences.reduce(
		(total, queue) => total + queue.itemCount,
		0,
	);
	return (
		<DeletionRows
			rows={[
				["Album", preview.albumTitle],
				[
					"Tracks",
					`${preview.trackCount} track${preview.trackCount === 1 ? "" : "s"}`,
				],
				["Total size", formatDeletionBytes(preview.totalSizeBytes)],
				[
					"Playlists",
					preview.playlistReferences
						.map((playlist) => playlist.name)
						.join(", ") || "None",
				],
				[
					"Queues",
					queueCount === 0
						? "None"
						: `${queueCount} Queue item${queueCount === 1 ? "" : "s"}`,
				],
			]}
		/>
	);
}
