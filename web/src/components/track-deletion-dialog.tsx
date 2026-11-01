import type { Track, TrackDeletionPreview } from "@repo/api-client";
import { Button } from "#/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogTitle,
} from "#/components/ui/dialog";

export function TrackDeletionDialog({
	track,
	preview,
	error,
	isLoading,
	isDeleting,
	onCancel,
	onConfirm,
	finalFocus,
}: {
	track: Track | null;
	preview: TrackDeletionPreview | null;
	error: string | null;
	isLoading: boolean;
	isDeleting: boolean;
	onCancel: () => void;
	onConfirm: () => void;
	finalFocus?: () => HTMLElement | true;
}) {
	return (
		<Dialog open={Boolean(track)} onOpenChange={(open) => !open && onCancel()}>
			<DialogContent finalFocus={finalFocus}>
				<DeletionHeading track={track} />
				{isLoading ? (
					<p className="mt-4 text-sm">Loading deletion details…</p>
				) : null}
				{preview ? <DeletionPreviewDetails preview={preview} /> : null}
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

function DeletionHeading({ track }: { track: Track | null }) {
	return (
		<>
			<DialogTitle>Permanently delete {track?.title}?</DialogTitle>
			<DialogDescription className="mt-2">
				This cannot be undone. No trash or restore copy will be kept.
			</DialogDescription>
		</>
	);
}

export function DeletionRows({ rows }: { rows: string[][] }) {
	return (
		<dl className="mt-4 space-y-3 text-sm">
			{rows.map(([label, value]) => (
				<div key={label} className="grid grid-cols-label gap-3">
					<dt className="text-caption">{label}</dt>
					<dd className="break-all text-foreground phone:break-normal phone:wrap-anywhere">
						{value}
					</dd>
				</div>
			))}
		</dl>
	);
}

function DeletionPreviewDetails({
	preview,
}: {
	preview: TrackDeletionPreview;
}) {
	const queueCount = preview.queueReferences.reduce(
		(total, queue) => total + queue.itemCount,
		0,
	);
	const rows = [
		["Track", preview.trackTitle],
		["Managed file", preview.managedFile.path],
		["File size", formatDeletionBytes(preview.managedFile.sizeBytes)],
		[
			"Playlists",
			preview.playlistReferences.map((playlist) => playlist.name).join(", ") ||
				"None",
		],
		[
			"Queues",
			queueCount === 0
				? "None"
				: `${queueCount} Queue item${queueCount === 1 ? "" : "s"}`,
		],
	];
	return <DeletionRows rows={rows} />;
}

export function DeletionActions({
	isReady,
	isDeleting,
	onCancel,
	onConfirm,
}: {
	isReady: boolean;
	isDeleting: boolean;
	onCancel: () => void;
	onConfirm: () => void;
}) {
	return (
		<DialogFooter className="mt-6">
			<Button
				type="button"
				variant="outline"
				disabled={isDeleting}
				onClick={onCancel}
			>
				Cancel
			</Button>
			<Button
				type="button"
				variant="destructive"
				disabled={!isReady || isDeleting}
				onClick={onConfirm}
			>
				{isDeleting ? "Deleting…" : "Delete permanently"}
			</Button>
		</DialogFooter>
	);
}

export function formatDeletionBytes(bytes: number): string {
	if (bytes === 0) return "0 bytes";
	return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}
