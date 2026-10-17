import {
	Check,
	ChevronDown,
	CircleAlert,
	CircleCheck,
	CircleX,
	Disc3,
	LoaderCircle,
	X,
} from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { type CSSProperties, memo, useId, useState } from "react";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import { isDesktopClient } from "#/desktop/bridge";
import { cn } from "#/lib/utils.ts";
import {
	ImportAlbumReview,
	ImportFileAlternatives,
} from "./-import-album-review";
import { ImportErrors } from "./-import-errors";
import { ImportTransferDetails, importPhaseLabel } from "./-import-progress";
import { normalizeImportTitle } from "./-import-review";
import {
	type DuplicateDecision,
	type ImportFileEntry,
	SUPPORTED_AUDIO_FILE_ACCEPT,
	summarizeImportEntries,
	useManagedImportWorkflow,
} from "./-managed-import-workflow";

export function ImportMusicDialog({
	isOpen,
	onOpenChange,
	onCommitted,
	onCloseAutoFocus,
}: {
	isOpen: boolean;
	onOpenChange: (isOpen: boolean) => void;
	onCommitted: () => Promise<void>;
	/** Restores focus to the opener; the dialog has no DialogTrigger. */
	onCloseAutoFocus?: (event: Event) => void;
}) {
	const workflow = useManagedImportWorkflow({ onOpenChange, onCommitted });

	return (
		<>
			{!isOpen && workflow.entries.length > 0 ? (
				<Button
					type="button"
					onClick={() => onOpenChange(true)}
					className="fixed bottom-24 right-6 z-40 shadow-lg"
					aria-label="Open current import"
				>
					{workflow.isCompleted
						? "Import results"
						: workflow.isBusy
							? "Import in progress"
							: "Ready for review"}
				</Button>
			) : null}
			<DialogPrimitive.Root
				open={isOpen}
				onOpenChange={workflow.handleOpenChange}
			>
				<DialogPrimitive.Portal>
					{/*
					 * The desktop client skips backdrop-blur: the dialog repaints on
					 * every progress tick, and a blurred full-screen backdrop makes
					 * WebKitGTK re-rasterize the whole window for each repaint.
					 */}
					<DialogPrimitive.Overlay
						className={cn(
							"fixed inset-0 z-50 bg-background/80",
							!isDesktopClient() && "backdrop-blur-sm",
						)}
					/>
					<DialogPrimitive.Content
						aria-describedby="import-music-description"
						onCloseAutoFocus={onCloseAutoFocus}
						className="-translate-x-1/2 -translate-y-1/2 fixed top-1/2 left-1/2 z-50 flex max-h-dialog-height w-dialog-width max-w-5xl flex-col overflow-hidden rounded-lg border border-border bg-background shadow-xl outline-none"
					>
						<ImportDialogHeader isBusy={false} />
						<div
							hidden={workflow.entries.length > 0}
							className={
								workflow.entries.length > 0
									? "hidden"
									: "grid gap-6 overflow-y-auto px-4 py-6 sm:px-6 scrollbar-hidden"
							}
						>
							<ImportFilePicker
								isBusy={workflow.isPickerLocked}
								onFiles={workflow.handleFiles}
								onDesktopSelection={workflow.handleDesktopSelection}
							/>
						</div>
						<ImportActivity errorMessage={workflow.errorMessage} />
						<ImportSummary
							entries={workflow.entries}
							isConfirming={workflow.isConfirming}
							isCompleted={workflow.isCompleted}
						/>
						{/* The single scroll region of the review step. Nested sections must
						 * not become scroll containers of their own: a non-overflowing
						 * `overflow-y-auto` + `overscroll-contain` child swallows wheel
						 * events and the dialog stops scrolling with the mouse. */}
						<div className="min-h-0 flex-1 overflow-y-auto overscroll-contain scrollbar-hidden">
							{!workflow.isCompleted && workflow.batchId ? (
								<ImportAlbumReview
									batchId={workflow.batchId}
									albums={workflow.albums}
									decisions={workflow.albumDecisions}
									isBusy={workflow.isSelectionLocked}
									onChange={workflow.handleAlbumDecision}
									onUploaded={workflow.handleArtworkUploaded}
									onUploadStateChange={workflow.handleArtworkUploadState}
								/>
							) : null}
							{!workflow.isCompleted ? (
								<ImportFileAlternatives
									entries={workflow.entries}
									isBusy={workflow.isSelectionLocked}
									onSelect={workflow.handleSelectionChange}
								/>
							) : null}
							{!workflow.isBusy && !workflow.isCompleted
								? workflow.reviewProblems.map((problem) => (
										<p
											key={problem}
											aria-live="polite"
											className="px-4 py-1 text-warning text-sm sm:px-6"
										>
											{problem}
										</p>
									))
								: null}
							<ImportFileList
								entries={workflow.entries}
								isBusy={workflow.isSelectionLocked}
								isConfirming={workflow.isConfirming}
								onSelectionChange={workflow.handleSelectionChange}
								onDuplicateDecisionChange={
									workflow.handleDuplicateDecisionChange
								}
							/>
						</div>
						<div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-border border-t px-4 py-3 sm:px-6">
							{workflow.canRetry ? (
								<Button
									type="button"
									variant="outline"
									onClick={() => void workflow.handleRetry()}
								>
									Retry failed uploads
								</Button>
							) : null}
							{workflow.canConfirm &&
							workflow.entries.some((entry) => entry.state !== "accepted") ? (
								<p className="text-caption text-sm">
									{
										workflow.entries.filter(
											(entry) => entry.state !== "accepted",
										).length
									}{" "}
									unsuccessful files will not be imported.
								</p>
							) : null}
							<ImportDialogFooter
								canConfirm={workflow.canConfirm}
								isBusy={workflow.isCloseLocked}
								isCompleted={workflow.isCompleted}
								onCancel={
									workflow.isCompleted
										? () => workflow.handleOpenChange(false)
										: workflow.handleCancel
								}
								onConfirm={workflow.handleConfirm}
							/>
						</div>
					</DialogPrimitive.Content>
				</DialogPrimitive.Portal>
			</DialogPrimitive.Root>
		</>
	);
}

function ImportDialogHeader({ isBusy }: { isBusy: boolean }) {
	return (
		<div className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-4 py-4 sm:px-6">
			<div>
				<DialogPrimitive.Title className="font-semibold text-heading text-xl">
					Import Music
				</DialogPrimitive.Title>
				<DialogPrimitive.Description
					id="import-music-description"
					className="mt-1 text-caption text-sm"
				>
					Upload audio files, review each result, then confirm selected Tracks.
				</DialogPrimitive.Description>
			</div>
			<DialogPrimitive.Close asChild>
				<Button type="button" variant="ghost" size="icon" disabled={isBusy}>
					<X className="size-4" />
					<span className="sr-only">Close Import Music</span>
				</Button>
			</DialogPrimitive.Close>
		</div>
	);
}

function ImportFilePicker({
	isBusy,
	onFiles,
	onDesktopSelection,
}: {
	isBusy: boolean;
	onFiles: (files: FileList) => Promise<void>;
	onDesktopSelection: (isDirectory: boolean) => Promise<void>;
}) {
	if (isDesktopClient()) {
		return (
			<div className="grid gap-4 sm:grid-cols-2">
				<Button
					type="button"
					variant="outline"
					disabled={isBusy}
					onClick={() => void onDesktopSelection(false)}
				>
					Select audio files
				</Button>
				<Button
					type="button"
					variant="outline"
					disabled={isBusy}
					onClick={() => void onDesktopSelection(true)}
				>
					Select audio folder
				</Button>
			</div>
		);
	}
	return (
		<div className="grid gap-4 sm:grid-cols-2">
			<ImportFileInput
				id="managed-import-files"
				label="Audio files"
				isBusy={isBusy}
				onFiles={onFiles}
			/>
			<ImportFileInput
				id="managed-import-folder"
				label="Audio folder"
				isBusy={isBusy}
				isDirectory
				onFiles={onFiles}
			/>
		</div>
	);
}

function ImportFileInput({
	id,
	label,
	isBusy,
	isDirectory = false,
	onFiles,
}: {
	id: string;
	label: string;
	isBusy: boolean;
	isDirectory?: boolean;
	onFiles: (files: FileList) => Promise<void>;
}) {
	return (
		<div className="grid gap-2">
			<label htmlFor={id} className="font-medium text-sm">
				{label}
			</label>
			<input
				ref={(input) => {
					if (isDirectory) input?.setAttribute("webkitdirectory", "");
				}}
				id={id}
				type="file"
				multiple
				accept={SUPPORTED_AUDIO_FILE_ACCEPT}
				disabled={isBusy}
				className="rounded-md border border-input bg-background px-3 py-2 text-sm file:mr-3 file:rounded-sm file:border-0 file:bg-secondary file:px-3 file:py-2 file:text-secondary-foreground"
				onChange={(event) =>
					event.target.files && void onFiles(event.target.files)
				}
			/>
		</div>
	);
}

function ImportActivity({ errorMessage }: { errorMessage: string }) {
	if (!errorMessage) return null;
	return (
		<p
			role="alert"
			className="mx-4 my-3 rounded-sm bg-destructive/10 px-3 py-2 text-destructive text-sm sm:mx-6"
		>
			{errorMessage}
		</p>
	);
}

function ImportSummary({
	entries,
	isConfirming,
	isCompleted,
}: {
	entries: ImportFileEntry[];
	isConfirming: boolean;
	isCompleted: boolean;
}) {
	if (entries.length === 0) return null;
	const summary = summarizeImportEntries(entries);
	const queued = entries.filter((entry) => entry.phase === "queued").length;
	const processing = entries.filter(
		(entry) => entry.state === "unresolved" && entry.phase !== "queued",
	).length;
	const selected = entries.filter((entry) => entry.selected).length;
	const imported = entries.filter(
		(entry) => entry.outcome === "imported" || entry.outcome === "replaced",
	).length;
	const percent = isCompleted
		? 100
		: isConfirming
			? selected > 0
				? Math.round((summary.completed / selected) * 100)
				: 0
			: summary.percent;
	return (
		<section
			aria-label="Import progress"
			className="shrink-0 space-y-2 border-border border-b px-4 py-3 sm:px-6"
		>
			<div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-sm">
				<p aria-live="polite" className="font-medium text-heading">
					{isCompleted
						? `Import complete · ${imported} imported`
						: isConfirming
							? `Importing ${summary.completed} of ${selected}`
							: `${summary.accepted} of ${summary.total} ready`}
				</p>
				<div className="flex flex-wrap gap-x-3 text-caption text-xs">
					{processing > 0 && !isConfirming ? (
						<SummaryCount count={processing} label="processing" />
					) : null}
					{queued > 0 && !isConfirming ? (
						<SummaryCount count={queued} label="queued" />
					) : null}
					{summary.needsReview > 0 ? (
						<SummaryCount count={summary.needsReview} label="need review" />
					) : null}
					{summary.rejected > 0 ? (
						<SummaryCount count={summary.rejected} label="unsuccessful" />
					) : null}
				</div>
			</div>
			<ProgressBar
				label={
					isConfirming ? "Importing selected tracks" : "Overall import progress"
				}
				percent={percent}
				className="h-1"
			/>
		</section>
	);
}

function ProgressBar({
	label,
	percent,
	isIndeterminate = false,
	className,
}: {
	label: string;
	percent: number;
	isIndeterminate?: boolean;
	className?: string;
}) {
	return (
		<div
			className={cn(
				"h-2 overflow-hidden rounded-full bg-primary/15",
				className,
			)}
			role="progressbar"
			aria-label={label}
			aria-valuenow={percent}
			aria-valuemin={0}
			aria-valuemax={100}
		>
			<div
				className={cn(
					"h-full w-(--progress) rounded-full bg-primary",
					isIndeterminate && "opacity-60",
				)}
				style={{ "--progress": `${percent}%` } as CSSProperties}
			/>
		</div>
	);
}

function SummaryCount({ count, label }: { count: number; label: string }) {
	return (
		<span>
			<span className="font-medium text-heading tabular-nums">{count}</span>{" "}
			{label}
		</span>
	);
}

type ImportAlbumGroup = {
	key: string;
	album: string;
	artists: string;
	entries: ImportFileEntry[];
};

function groupImportEntries(entries: ImportFileEntry[]): ImportAlbumGroup[] {
	const groups = new Map<string, ImportAlbumGroup>();
	for (const entry of entries) {
		const file = entry.preview?.file;
		const album = file?.album ?? "Selected files";
		const artists = (file?.albumArtists ?? file?.artists ?? []).join(", ");
		const key = JSON.stringify([album, artists]);
		let group = groups.get(key);
		if (!group) {
			group = { key, album, artists, entries: [] };
			groups.set(key, group);
		}
		group.entries.push(entry);
	}
	return Array.from(groups.values());
}

function ImportFileList({
	entries,
	isBusy,
	isConfirming,
	onSelectionChange,
	onDuplicateDecisionChange,
}: {
	entries: ImportFileEntry[];
	isBusy: boolean;
	isConfirming: boolean;
	onSelectionChange: (key: string, selected: boolean) => void;
	onDuplicateDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	if (entries.length === 0) return null;
	return (
		<section
			aria-label="Import Preview"
			className="outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
		>
			<h3 className="sr-only">Import Preview</h3>
			{groupImportEntries(entries).flatMap((group) => [
				<ImportAlbumHeader key={`album-${group.key}`} group={group} />,
				...group.entries.map((entry) => (
					<ImportFileRow
						key={entry.key}
						entry={entry}
						albumArtists={group.artists}
						isBusy={isBusy}
						isConfirming={isConfirming}
						onSelectionChange={onSelectionChange}
						onDuplicateDecisionChange={onDuplicateDecisionChange}
					/>
				)),
			])}
		</section>
	);
}

function ImportAlbumHeader({ group }: { group: ImportAlbumGroup }) {
	return (
		<div className="flex items-center gap-3 bg-muted/30 px-4 py-3 sm:px-6">
			<Disc3 aria-hidden="true" className="size-8 shrink-0 text-caption" />
			<div className="min-w-0 flex-1">
				<h4 className="break-words font-medium text-heading text-sm">
					{group.album}
				</h4>
				{group.artists ? (
					<p className="break-words text-caption text-xs">{group.artists}</p>
				) : null}
			</div>
			<span className="shrink-0 text-caption text-xs tabular-nums">
				{group.entries.length} {group.entries.length === 1 ? "track" : "tracks"}
			</span>
		</div>
	);
}

// Memoized so a progress tick on one upload only re-renders that row.
const ImportFileRow = memo(function ImportFileRow({
	entry,
	albumArtists,
	isConfirming,
	isBusy,
	onSelectionChange,
	onDuplicateDecisionChange,
}: {
	entry: ImportFileEntry;
	albumArtists: string;
	isConfirming: boolean;
	isBusy: boolean;
	onSelectionChange: (key: string, selected: boolean) => void;
	onDuplicateDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	const [isExpanded, setIsExpanded] = useState(false);
	const detailsId = useId();
	const filename = entry.preview?.file.originalFilename ?? entry.file.name;
	const duplicateClassification =
		entry.preview?.duplicateClassification ?? "none";
	const isUploading =
		entry.phase === "uploading" ||
		(!entry.phase && entry.state === "unresolved" && entry.progress < 100);
	return (
		<article className="border-border/50 border-b px-4 py-2 transition-colors hover:bg-muted/20 sm:px-6">
			<div className="flex min-h-10 items-center gap-3">
				{duplicateClassification === "none" ? (
					<SelectionCheckbox
						label={`Select ${filename}`}
						checked={entry.selected}
						disabled={isBusy || entry.state !== "accepted"}
						onChange={(selected) => onSelectionChange(entry.key, selected)}
					/>
				) : (
					<DuplicateMarker classification={duplicateClassification} />
				)}
				<span className="min-w-0 flex-1">
					<span
						className={cn(
							"block text-heading text-sm",
							isExpanded ? "break-words" : "truncate",
						)}
						title={entry.preview?.file.title ?? filename}
					>
						{entry.preview?.file.title ?? filename}
					</span>
					<RowCaption
						entry={entry}
						albumArtists={albumArtists}
						isExpanded={isExpanded}
					/>
				</span>
				<span className="flex shrink-0 items-center gap-2">
					{isUploading ? (
						<span className="text-caption text-xs tabular-nums">
							{entry.progress}%
						</span>
					) : null}
					<StatusBadge entry={entry} isConfirming={isConfirming} />
					<Button
						type="button"
						variant="ghost"
						size="icon"
						className="size-8 text-caption"
						aria-label={`Details for ${filename}`}
						aria-expanded={isExpanded}
						aria-controls={isExpanded ? detailsId : undefined}
						onClick={() => setIsExpanded(!isExpanded)}
					>
						<ChevronDown
							aria-hidden="true"
							className={cn(
								"size-4 transition-transform",
								isExpanded && "rotate-180",
							)}
						/>
					</Button>
				</span>
			</div>
			{isUploading ? (
				<ProgressBar
					label={`${filename} upload progress`}
					percent={entry.progress}
					className="h-1.5"
				/>
			) : null}
			{entry.phase === "uploading" || entry.phase === "retry_wait" ? (
				<ImportTransferDetails entry={entry} />
			) : null}
			{isExpanded ? (
				<div
					id={detailsId}
					className="ml-8 space-y-2 border-border/50 border-t py-3 text-caption text-xs"
				>
					<p className="break-all">
						<span className="font-medium">File:</span> {filename}
					</p>
					<ImportTransferDetails entry={entry} isExpanded />
				</div>
			) : null}
			<DuplicateReview
				entry={entry}
				isBusy={isBusy}
				onDecisionChange={onDuplicateDecisionChange}
			/>
			<ImportErrors message={entry.errorMessage} />
		</article>
	);
});

function RowCaption({
	entry,
	albumArtists,
	isExpanded,
}: {
	entry: ImportFileEntry;
	albumArtists: string;
	isExpanded: boolean;
}) {
	const artists = entry.preview?.file.artists.join(", ");
	if (!artists || artists === albumArtists) return null;
	return (
		<span
			className={cn(
				"block text-caption text-xs",
				isExpanded ? "break-words" : "truncate",
			)}
			title={artists}
		>
			{artists}
		</span>
	);
}

function SelectionCheckbox({
	label,
	checked,
	disabled,
	onChange,
}: {
	label: string;
	checked: boolean;
	disabled: boolean;
	onChange: (checked: boolean) => void;
}) {
	return (
		<span className="relative flex size-4 shrink-0 items-center justify-center">
			<input
				type="checkbox"
				aria-label={label}
				checked={checked}
				disabled={disabled}
				onChange={(event) => onChange(event.target.checked)}
				className="peer size-4 cursor-pointer appearance-none rounded-sm border border-input bg-background transition-colors checked:border-primary checked:bg-primary focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-40"
			/>
			<Check
				aria-hidden="true"
				className="pointer-events-none absolute size-3 text-primary-foreground opacity-0 peer-checked:opacity-100"
			/>
		</span>
	);
}

function DuplicateMarker({
	classification,
}: {
	classification: "exact_duplicate";
}) {
	const Icon = classification === "exact_duplicate" ? CircleX : CircleAlert;
	return (
		<Icon
			aria-hidden="true"
			className={cn(
				"size-4 shrink-0",
				classification === "exact_duplicate"
					? "text-destructive"
					: "text-caption",
			)}
		/>
	);
}

function StatusBadge({
	entry,
	isConfirming,
}: {
	entry: ImportFileEntry;
	isConfirming: boolean;
}) {
	if (isConfirming && entry.selected && !entry.outcome) {
		return (
			<span className="flex items-center gap-2 text-caption text-xs">
				<LoaderCircle aria-hidden="true" className="size-3 animate-spin" />
				Importing…
			</span>
		);
	}
	const badge = entry.outcome
		? outcomeBadges[entry.outcome]
		: stateBadges[entry.state];
	return (
		<Badge
			variant={badge.variant}
			className={cn(
				"border-0 bg-transparent px-0 font-normal text-xs dark:bg-transparent",
				badge.variant === "destructive" ? "text-destructive" : "text-caption",
			)}
		>
			<badge.Icon aria-hidden="true" className={badge.iconClassName} />
			{entry.state === "unresolved" ? importPhaseLabel(entry) : badge.label}
		</Badge>
	);
}

type StatusBadgeSpec = {
	label: string;
	variant: "default" | "secondary" | "destructive" | "outline";
	iconClassName?: string;
	Icon: typeof Check;
};

const stateBadges: Record<ImportFileEntry["state"], StatusBadgeSpec> = {
	accepted: { label: "Accepted", variant: "outline", Icon: CircleCheck },
	rejected: { label: "Rejected", variant: "destructive", Icon: CircleX },
	unresolved: { label: "Unresolved", variant: "secondary", Icon: LoaderCircle },
	completed: { label: "Completed", variant: "default", Icon: CircleCheck },
};

const outcomeBadges: Record<
	NonNullable<ImportFileEntry["outcome"]>,
	StatusBadgeSpec
> = {
	imported: { label: "Imported", variant: "default", Icon: CircleCheck },
	replaced: { label: "Replaced", variant: "default", Icon: CircleCheck },
	rejected: { label: "Rejected", variant: "destructive", Icon: CircleX },
	failed: { label: "Failed", variant: "destructive", Icon: CircleX },
	not_attempted: {
		label: "Not attempted",
		variant: "secondary",
		Icon: CircleAlert,
	},
};

function DuplicateReview({
	entry,
	isBusy,
	onDecisionChange,
}: {
	entry: ImportFileEntry;
	isBusy: boolean;
	onDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	const preview = entry.preview;
	if (!preview) return null;
	if (preview.duplicateClassification === "exact_duplicate")
		return <ExactDuplicateReview preview={preview} />;
	const candidates = preview.matchingTracks ?? [];
	if (!candidates.length)
		return preview.file.artworkWarning ? (
			<p className="text-warning text-sm">{preview.file.artworkWarning}</p>
		) : null;
	if (
		candidates.some(
			(candidate) =>
				(candidate.titleKey ?? normalizeImportTitle(candidate.title)) !==
				(preview.file.titleKey ?? normalizeImportTitle(preview.file.title)),
		)
	)
		return (
			<p className="text-destructive text-sm">
				Another title occupies this Album position. Skip this file or create a
				separate Album.
			</p>
		);
	return (
		<fieldset className="grid gap-2 rounded-sm border border-border p-3">
			<legend className="px-1 font-medium">Track Replacement</legend>
			<p className="text-caption text-sm">
				Replace {candidates[0]?.title} ({candidates[0]?.format}) with this{" "}
				{preview.file.format} file? Track identity and playlist references will
				be preserved.
			</p>
			<p className="text-caption text-sm">
				Artist credits: {candidates[0]?.artists.join(", ")} / incoming:{" "}
				{preview.file.artists.join(", ")}
			</p>
			<ReplacementMetadataChanges preview={preview} />
			{duplicateDecisionOptions.map((option) => (
				<DuplicateDecisionOption
					key={option.value}
					entry={entry}
					isBusy={isBusy}
					option={option}
					onDecisionChange={onDecisionChange}
				/>
			))}
		</fieldset>
	);
}

function ExactDuplicateReview({
	preview,
}: {
	preview: NonNullable<ImportFileEntry["preview"]>;
}) {
	const candidate = preview.duplicateCandidates?.[0];
	if (!candidate) return null;
	return (
		<div className="rounded-md border border-border bg-background p-3 text-sm">
			<p className="font-medium text-heading">Exact Duplicate</p>
			<p className="mt-1 text-caption">
				File bytes already belong to {candidate.title}.
			</p>
			<details className="mt-2">
				<summary className="cursor-pointer text-primary underline">
					View existing Track
				</summary>
				<dl className="mt-2 grid grid-cols-media gap-x-3 gap-y-1 text-caption">
					<dt>Artist</dt>
					<dd>{candidate.artists.join(", ")}</dd>
					<dt>Album</dt>
					<dd>{candidate.album}</dd>
					<dt>Position</dt>
					<dd>{`${candidate.discNo}.${candidate.trackNo}`}</dd>
				</dl>
			</details>
		</div>
	);
}

function DuplicateDecisionOption({
	entry,
	isBusy,
	option,
	onDecisionChange,
}: {
	entry: ImportFileEntry;
	isBusy: boolean;
	option: (typeof duplicateDecisionOptions)[number];
	onDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	return (
		<label className="flex items-center gap-2 text-sm">
			<input
				type="radio"
				name={`duplicate-decision-${entry.key}`}
				value={option.value}
				checked={entry.duplicateDecision === option.value}
				disabled={isBusy}
				onChange={() => onDecisionChange(entry.key, option.value)}
			/>
			{option.label}
		</label>
	);
}

const duplicateDecisionOptions: Array<{
	value: DuplicateDecision;
	label: string;
}> = [
	{ value: "replace_existing", label: "Replace existing Track" },
	{ value: "do_not_import", label: "Do not import" },
];

function ImportDialogFooter({
	canConfirm,
	isBusy,
	isCompleted,
	onCancel,
	onConfirm,
}: {
	canConfirm: boolean;
	isBusy: boolean;
	isCompleted: boolean;
	onCancel: () => void;
	onConfirm: () => Promise<void>;
}) {
	return (
		<div className="ml-auto flex shrink-0 justify-end gap-2">
			<Button
				type="button"
				variant="outline"
				disabled={isBusy}
				onClick={onCancel}
			>
				{isCompleted ? "Done" : "Cancel"}
			</Button>
			{!isCompleted ? (
				<Button
					type="button"
					disabled={!canConfirm}
					onClick={() => void onConfirm()}
				>
					Confirm Import
				</Button>
			) : null}
		</div>
	);
}

function ReplacementMetadataChanges({
	preview,
}: {
	preview: NonNullable<ImportFileEntry["preview"]>;
}) {
	const current = preview.matchingTracks?.[0]?.currentFile;
	if (!current) return null;
	const incoming = preview.file;
	const fields = [
		"title",
		"artists",
		"albumArtists",
		"album",
		"year",
		"genres",
		"discNo",
		"trackNo",
		"discTotal",
		"trackTotal",
		"format",
		"container",
		"codec",
		"durationMs",
		"sampleRateHz",
		"channelCount",
		"bitDepth",
		"bitrateKbps",
		"sizeBytes",
	] as const;
	const read = (file: typeof current, field: string): string => {
		const value = Reflect.get(file, field);
		return Array.isArray(value)
			? value.join(", ")
			: value == null || value === ""
				? "Not tagged"
				: String(value);
	};
	const changes = fields
		.map((field) => ({
			field,
			previous: read(current, field),
			incoming: read(incoming, field),
		}))
		.filter((change) => change.previous !== change.incoming);
	return (
		<details>
			<summary className="cursor-pointer text-sm">
				Review metadata and audio changes ({changes.length})
			</summary>
			<table className="w-full text-left text-sm">
				<thead>
					<tr>
						<th>Field</th>
						<th>Existing</th>
						<th>Incoming</th>
					</tr>
				</thead>
				<tbody>
					{changes.map((change) => (
						<tr key={change.field}>
							<th>{change.field}</th>
							<td>{change.previous}</td>
							<td>{change.incoming}</td>
						</tr>
					))}
				</tbody>
			</table>
		</details>
	);
}
