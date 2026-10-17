import type { Track, TrackDeletionPreview } from "@repo/api-client";
import { Dialog as DialogPrimitive } from "radix-ui";

export const DELETION_DIALOG_OVERLAY_CLASS =
	"fixed inset-0 z-50 bg-background/70";
export const DELETION_DIALOG_CONTENT_CLASS =
	"-translate-x-1/2 -translate-y-1/2 fixed top-1/2 left-1/2 z-50 w-dialog-width max-w-lg rounded-md border border-border bg-popover p-6 text-popover-foreground shadow-xl outline-none";

export function TrackDeletionDialog({
	track,
	preview,
	error,
	isLoading,
	isDeleting,
	onCancel,
	onConfirm,
	onCloseAutoFocus,
}: {
	track: Track | null;
	preview: TrackDeletionPreview | null;
	error: string | null;
	isLoading: boolean;
	isDeleting: boolean;
	onCancel: () => void;
	onConfirm: () => void;
	onCloseAutoFocus?: (event: Event) => void;
}) {
	return (
		<DialogPrimitive.Root
			open={Boolean(track)}
			onOpenChange={(open) => !open && onCancel()}
		>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Overlay className={DELETION_DIALOG_OVERLAY_CLASS} />
				<DialogPrimitive.Content
					onCloseAutoFocus={onCloseAutoFocus}
					className={DELETION_DIALOG_CONTENT_CLASS}
				>
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
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}

function DeletionHeading({ track }: { track: Track | null }) {
	return (
		<>
			<DialogPrimitive.Title className="font-semibold text-heading text-xl">
				Permanently delete {track?.title}?
			</DialogPrimitive.Title>
			<DialogPrimitive.Description className="mt-2 text-caption text-sm">
				This cannot be undone. No trash or restore copy will be kept.
			</DialogPrimitive.Description>
		</>
	);
}

export function DeletionRows({ rows }: { rows: string[][] }) {
	return (
		<dl className="mt-4 space-y-3 text-sm">
			{rows.map(([label, value]) => (
				<div key={label} className="grid grid-cols-label gap-3">
					<dt className="text-caption">{label}</dt>
					<dd className="break-all text-foreground">{value}</dd>
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
		<div className="mt-6 flex justify-end gap-2">
			<button
				type="button"
				className="rounded-sm border border-border px-4 py-2 text-sm"
				disabled={isDeleting}
				onClick={onCancel}
			>
				Cancel
			</button>
			<button
				type="button"
				className="rounded-sm bg-destructive px-4 py-2 text-destructive-foreground text-sm"
				disabled={!isReady || isDeleting}
				onClick={onConfirm}
			>
				{isDeleting ? "Deleting…" : "Delete permanently"}
			</button>
		</div>
	);
}

export function formatDeletionBytes(bytes: number): string {
	if (bytes === 0) return "0 bytes";
	return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}
