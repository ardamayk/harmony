import type {
	Track,
	TrackReplacementFieldDiff,
	TrackReplacementPreview,
} from "@repo/api-client";
import { Dialog as DialogPrimitive } from "radix-ui";
import type { TrackReplacementStep } from "#/hooks/use-track-replacement-flow";
import { SUPPORTED_AUDIO_FILE_ACCEPT } from "#/routes/library/tracks/-managed-import-workflow";

export function TrackReplacementDialog({
	track,
	step,
	preview,
	progress,
	error,
	isBusy,
	isDesktop,
	onCancel,
	onClose,
	onFile,
	onSelectDesktopFile,
	onConfirm,
	onCloseAutoFocus,
}: {
	track: Track | null;
	step: TrackReplacementStep;
	preview: TrackReplacementPreview | null;
	progress: number;
	error: string | null;
	isBusy: boolean;
	isDesktop: boolean;
	onCancel: () => void;
	onClose: () => void;
	onFile: (file: File) => void;
	onSelectDesktopFile: () => void;
	onConfirm: () => void;
	onCloseAutoFocus?: (event: Event) => void;
}) {
	return (
		<DialogPrimitive.Root
			open={Boolean(track)}
			onOpenChange={(open) => {
				if (open) return;
				if (step === "completed") onClose();
				else onCancel();
			}}
		>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-background/70" />
				<DialogPrimitive.Content
					aria-describedby="track-replacement-description"
					onCloseAutoFocus={onCloseAutoFocus}
					className="-translate-x-1/2 -translate-y-1/2 fixed top-1/2 left-1/2 z-50 max-h-dialog-height w-dialog-width max-w-2xl overflow-y-auto rounded-md border border-border bg-popover p-6 text-popover-foreground shadow-xl outline-none"
				>
					<DialogPrimitive.Title className="font-semibold text-heading text-xl">
						Replace {track?.title}
					</DialogPrimitive.Title>
					<DialogPrimitive.Description
						id="track-replacement-description"
						className="mt-2 text-caption text-sm"
					>
						The Track keeps its identity, Playlist and Queue references. The
						previous managed file is deleted only after the replacement is
						verified.
					</DialogPrimitive.Description>
					{step === "select" || step === "uploading" ? (
						<ReplacementFilePicker
							isDesktop={isDesktop}
							isBusy={isBusy}
							progress={progress}
							step={step}
							onFile={onFile}
							onSelectDesktopFile={onSelectDesktopFile}
						/>
					) : null}
					{preview && step !== "completed" ? (
						<ReplacementReview preview={preview} />
					) : null}
					{step === "completed" ? (
						<output className="mt-4 block text-sm">
							{track?.title} was replaced. Playback of this Track was stopped if
							it was active.
						</output>
					) : null}
					{error ? (
						<p role="alert" className="mt-4 text-destructive text-sm">
							{error}
						</p>
					) : null}
					<ReplacementActions
						step={step}
						isBusy={isBusy}
						onCancel={onCancel}
						onClose={onClose}
						onConfirm={onConfirm}
					/>
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}

function ReplacementFilePicker({
	isDesktop,
	isBusy,
	progress,
	step,
	onFile,
	onSelectDesktopFile,
}: {
	isDesktop: boolean;
	isBusy: boolean;
	progress: number;
	step: TrackReplacementStep;
	onFile: (file: File) => void;
	onSelectDesktopFile: () => void;
}) {
	return (
		<div className="mt-4 grid gap-2">
			{isDesktop ? (
				<button
					type="button"
					className="rounded-sm border border-border px-4 py-2 text-sm"
					disabled={isBusy}
					onClick={onSelectDesktopFile}
				>
					Select replacement file
				</button>
			) : (
				<>
					<label
						htmlFor="track-replacement-file"
						className="font-medium text-sm"
					>
						Replacement audio file
					</label>
					<input
						id="track-replacement-file"
						type="file"
						accept={SUPPORTED_AUDIO_FILE_ACCEPT}
						disabled={isBusy}
						className="rounded-md border border-input bg-background px-3 py-2 text-sm file:mr-3 file:rounded-sm file:border-0 file:bg-secondary file:px-3 file:py-2 file:text-secondary-foreground"
						onChange={(event) => {
							const file = event.target.files?.[0];
							if (file) onFile(file);
						}}
					/>
				</>
			)}
			{step === "uploading" ? (
				<div
					className="h-2 overflow-hidden rounded-full bg-secondary"
					role="progressbar"
					aria-label="Replacement upload progress"
					aria-valuenow={progress}
					aria-valuemin={0}
					aria-valuemax={100}
				>
					<div
						className="h-full bg-primary transition-all"
						style={{ width: `${progress}%` }}
					/>
				</div>
			) : null}
			{step === "uploading" ? (
				<p aria-live="polite" className="text-caption text-sm">
					Uploading and validating the replacement…
				</p>
			) : null}
		</div>
	);
}

function ReplacementReview({ preview }: { preview: TrackReplacementPreview }) {
	const queueCount = preview.queueReferences.reduce(
		(total, queue) => total + queue.itemCount,
		0,
	);
	return (
		<section aria-label="Track Replacement review" className="mt-4 grid gap-4">
			<DiffSection title="Source format" diffs={[preview.sourceFormat]} />
			<DiffSection
				title="Technical properties"
				diffs={preview.technicalProperties}
			/>
			<DiffSection title="Metadata" diffs={preview.metadata} />
			<LibrarySection preview={preview} />
			<ArtworkSection preview={preview} />
			<DiffSection title="Canonical path" diffs={[preview.canonicalPath]} />
			<div className="rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm">
				<p className="font-medium text-heading">Old file deletion</p>
				<p className="mt-1 break-all text-caption">
					{preview.oldFile.path} (
					{formatReplacementBytes(preview.oldFile.sizeBytes)}) will be deleted
					permanently after the replacement is verified.
				</p>
			</div>
			<dl className="grid grid-cols-label gap-3 text-sm">
				<dt className="text-caption">Playlists kept</dt>
				<dd>
					{preview.playlistReferences
						.map((playlist) => playlist.name)
						.join(", ") || "None"}
				</dd>
				<dt className="text-caption">Queues kept</dt>
				<dd>
					{queueCount === 0
						? "None"
						: `${queueCount} Queue item${queueCount === 1 ? "" : "s"}`}
				</dd>
			</dl>
		</section>
	);
}

function DiffSection({
	title,
	diffs,
}: {
	title: string;
	diffs: TrackReplacementFieldDiff[];
}) {
	const changed = diffs.filter((diff) => diff.isChanged);
	return (
		<div>
			<h3 className="font-medium text-heading text-sm">{title}</h3>
			{changed.length === 0 ? (
				<p className="text-caption text-sm">No changes</p>
			) : (
				<dl className="mt-1 grid grid-cols-label gap-x-3 gap-y-1 text-sm">
					{changed.map((diff) => (
						<DiffRow key={diff.field} diff={diff} />
					))}
				</dl>
			)}
		</div>
	);
}

function DiffRow({ diff }: { diff: TrackReplacementFieldDiff }) {
	return (
		<>
			<dt className="text-caption">{diff.field}</dt>
			<dd className="break-all">
				<span className="line-through opacity-70">{diff.current || "—"}</span>
				{" → "}
				<span className="font-medium text-foreground">
					{diff.replacement || "—"}
				</span>
			</dd>
		</>
	);
}

function LibrarySection({ preview }: { preview: TrackReplacementPreview }) {
	const { library } = preview;
	const notes = [
		library.movesAlbum
			? library.createsAlbum
				? "Moves the Track into a new Album"
				: "Moves the Track into an existing Album"
			: null,
		library.removesEmptyAlbum ? "Removes the emptied Album" : null,
		library.removesEmptyArtists.length > 0
			? `Removes unreferenced Artists: ${library.removesEmptyArtists.join(", ")}`
			: null,
		library.createsArtists.length > 0
			? `Creates Artists: ${library.createsArtists.join(", ")}`
			: null,
		library.createsGenres.length > 0
			? `Creates Genres: ${library.createsGenres.join(", ")}`
			: null,
	].filter((note): note is string => Boolean(note));
	return (
		<div>
			<h3 className="font-medium text-heading text-sm">Album and Artist</h3>
			{notes.length === 0 ? (
				<p className="text-caption text-sm">No changes</p>
			) : (
				<ul className="list-disc pl-6 text-sm">
					{notes.map((note) => (
						<li key={note}>{note}</li>
					))}
				</ul>
			)}
		</div>
	);
}

function ArtworkSection({ preview }: { preview: TrackReplacementPreview }) {
	const { artwork } = preview;
	return (
		<div>
			<h3 className="font-medium text-heading text-sm">Artwork</h3>
			{artwork.isChanged ? (
				<p className="text-sm">
					Embedded artwork changes from {artwork.currentMediaType || "none"} to{" "}
					{artwork.replacementMediaType}
					{artwork.replacesAlbumArtwork
						? " and replaces the Album artwork"
						: ""}
					.
				</p>
			) : (
				<p className="text-caption text-sm">No changes</p>
			)}
		</div>
	);
}

function ReplacementActions({
	step,
	isBusy,
	onCancel,
	onClose,
	onConfirm,
}: {
	step: TrackReplacementStep;
	isBusy: boolean;
	onCancel: () => void;
	onClose: () => void;
	onConfirm: () => void;
}) {
	if (step === "completed") {
		return (
			<div className="mt-6 flex justify-end gap-2">
				<button
					type="button"
					className="rounded-sm border border-border px-4 py-2 text-sm"
					onClick={onClose}
				>
					Done
				</button>
			</div>
		);
	}
	return (
		<div className="mt-6 flex justify-end gap-2">
			<button
				type="button"
				className="rounded-sm border border-border px-4 py-2 text-sm"
				disabled={step === "replacing"}
				onClick={onCancel}
			>
				Cancel
			</button>
			<button
				type="button"
				className="rounded-sm bg-destructive px-4 py-2 text-destructive-foreground text-sm"
				disabled={step !== "review" || isBusy}
				onClick={onConfirm}
			>
				{step === "replacing" ? "Replacing…" : "Replace track"}
			</button>
		</div>
	);
}

function formatReplacementBytes(bytes: number): string {
	if (bytes === 0) return "0 bytes";
	return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}
