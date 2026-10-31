import {
	ChevronDown,
	Circle,
	CircleAlert,
	CircleCheck,
	CircleX,
	Disc3,
	LoaderCircle,
	type LucideIcon,
	X,
} from "lucide-react";
import { toast } from "@repo/ui";
import {
	type CSSProperties,
	Fragment,
	memo,
	type ReactNode,
	useEffect,
	useId,
	useRef,
	useState,
} from "react";
import { Button } from "#/components/ui/button";
import { Checkbox } from "#/components/ui/checkbox";
import {
	Dialog,
	DialogClose,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "#/components/ui/dialog";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { RadioGroup, RadioGroupItem } from "#/components/ui/radio-group";
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
	summarizeImportResult,
	useManagedImportWorkflow,
} from "./-managed-import-workflow";

export function ImportMusicDialog({
	isOpen,
	onOpenChange,
	onCommitted,
	finalFocus,
}: {
	isOpen: boolean;
	onOpenChange: (isOpen: boolean) => void;
	onCommitted: () => Promise<void>;
	/** Restores focus to the opener; the dialog has no DialogTrigger. */
	finalFocus?: () => HTMLElement | true;
}) {
	const workflow = useManagedImportWorkflow({ onOpenChange, onCommitted });
	const { isConfirming, isCompleted, entries, handleCancel } = workflow;
	const isProcessing = entries.some((entry) => entry.state === "unresolved");
	const wasConfirming = useRef(false);
	// An open dialog shows its own results; a closed one ends with a toast.
	useEffect(() => {
		if (isConfirming) {
			wasConfirming.current = true;
			return;
		}
		if (!wasConfirming.current || !isCompleted) return;
		wasConfirming.current = false;
		if (isOpen) return;
		const { level, message } = summarizeImportResult(entries);
		toast[level](message);
		void handleCancel();
	}, [isConfirming, isCompleted, isOpen, entries, handleCancel]);

	return (
		<>
			{!isOpen && workflow.entries.length > 0 ? (
				<Button
					type="button"
					onClick={() => onOpenChange(true)}
					className="fixed bottom-24 right-6 z-40"
					aria-label="Open current import"
				>
					{workflow.isCompleted
						? "Import results"
						: workflow.isConfirming
							? "Importing…"
							: workflow.isBusy
								? "Import in progress"
								: "Ready for review"}
				</Button>
			) : null}
			<Dialog open={isOpen} onOpenChange={workflow.handleOpenChange}>
				{/*
				 * The desktop client skips backdrop-blur: the dialog repaints on
				 * every progress tick, and a blurred full-screen backdrop makes
				 * WebKitGTK re-rasterize the whole window for each repaint.
				 */}
				<DialogContent
					finalFocus={finalFocus}
					overlayClassName={cn(
						"bg-background/80",
						!isDesktopClient() && "backdrop-blur-sm",
					)}
					className="flex max-h-dialog-height max-w-5xl flex-col overflow-hidden bg-background p-0 text-foreground"
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
						{!workflow.isCompleted && !isProcessing ? (
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
							isBusy={workflow.isSelectionLocked || workflow.isCompleted}
							isConfirming={workflow.isConfirming}
							isProcessing={isProcessing}
							renderAlbumHeader={(group) => {
								const album = workflow.albums.find(
									(candidate) => candidate.key === group.key,
								);
								return album && workflow.batchId && !workflow.isCompleted ? (
									<ImportAlbumReview
										batchId={workflow.batchId}
										album={album}
										decisions={workflow.albumDecisions}
										trackCount={group.entries.length}
										isBusy={workflow.isSelectionLocked}
										onChange={workflow.handleAlbumDecision}
										onUploaded={workflow.handleArtworkUploaded}
										onUploadStateChange={workflow.handleArtworkUploadState}
									/>
								) : (
									<ImportAlbumHeader group={group} />
								);
							}}
							onSelectionChange={workflow.handleSelectionChange}
							onDuplicateDecisionChange={workflow.handleDuplicateDecisionChange}
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
						{workflow.canConfirm ? (
							<p className="text-caption text-sm">
								{workflow.entries.some((entry) => entry.state !== "accepted")
									? `${
											workflow.entries.filter(
												(entry) => entry.state !== "accepted",
											).length
										} unsuccessful files will not be imported. `
									: ""}
								Nothing is added to your library until you confirm.
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
				</DialogContent>
			</Dialog>
		</>
	);
}

function ImportDialogHeader({ isBusy }: { isBusy: boolean }) {
	return (
		<div className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-4 py-4 sm:px-6">
			<div>
				<DialogTitle>Import Music</DialogTitle>
				<DialogDescription className="mt-1">
					Upload audio files, review each result, then confirm selected Tracks.
				</DialogDescription>
			</div>
			<DialogClose
				render={
					<Button type="button" variant="ghost" size="icon" disabled={isBusy} />
				}
			>
				<X className="size-4" />
				<span className="sr-only">Close Import Music</span>
			</DialogClose>
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
			<Label htmlFor={id}>{label}</Label>
			<Input
				ref={(input) => {
					if (isDirectory) input?.setAttribute("webkitdirectory", "");
				}}
				id={id}
				type="file"
				multiple
				accept={SUPPORTED_AUDIO_FILE_ACCEPT}
				disabled={isBusy}
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
		const key = file?.albumKey ?? JSON.stringify([album, artists]);
		let group = groups.get(key);
		if (!group) {
			group = { key, album, artists, entries: [] };
			groups.set(key, group);
		}
		group.entries.push(entry);
	}
	return Array.from(groups.values());
}

export function ImportFileList({
	entries,
	isBusy,
	isConfirming,
	isProcessing,
	renderAlbumHeader,
	onSelectionChange,
	onDuplicateDecisionChange,
}: {
	entries: ImportFileEntry[];
	isBusy: boolean;
	isConfirming: boolean;
	isProcessing: boolean;
	renderAlbumHeader: (group: ImportAlbumGroup) => ReactNode;
	onSelectionChange: (key: string, selected: boolean) => void;
	onDuplicateDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	if (entries.length === 0) return null;
	const renderRow = (entry: ImportFileEntry, albumArtists: string) => (
		<ImportFileRow
			key={entry.key}
			entry={entry}
			albumArtists={albumArtists}
			isBusy={isBusy}
			isConfirming={isConfirming}
			isProcessing={isProcessing}
			onSelectionChange={onSelectionChange}
			onDuplicateDecisionChange={onDuplicateDecisionChange}
		/>
	);
	return (
		<section
			aria-label="Import Preview"
			className="outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
		>
			<h3 className="sr-only">Import Preview</h3>
			{/* Rows hold their place while files are processed: grouping them
			 * as each preview arrives would push the list around. */}
			{isProcessing
				? entries.map((entry) => renderRow(entry, ""))
				: groupImportEntries(entries).flatMap((group) => [
						<Fragment key={`album-${group.key}`}>
							{renderAlbumHeader(group)}
						</Fragment>,
						...group.entries.map((entry) => renderRow(entry, group.artists)),
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
	isProcessing,
	onSelectionChange,
	onDuplicateDecisionChange,
}: {
	entry: ImportFileEntry;
	albumArtists: string;
	isConfirming: boolean;
	isBusy: boolean;
	isProcessing: boolean;
	onSelectionChange: (key: string, selected: boolean) => void;
	onDuplicateDecisionChange: (key: string, decision: DuplicateDecision) => void;
}) {
	const [isExpanded, setIsExpanded] = useState(false);
	const detailsId = useId();
	const filename = entry.preview?.file.originalFilename ?? entry.file.name;
	const status = importStatus(entry, isConfirming);
	const canSelect =
		entry.state === "accepted" &&
		!isProcessing &&
		!(isConfirming && entry.selected);
	const isUploading =
		entry.phase === "uploading" ||
		(!entry.phase && entry.state === "unresolved" && entry.progress < 100);
	return (
		<article className="relative border-border/50 border-b px-4 py-2 transition-colors hover:bg-muted/20 sm:px-6">
			<div className="flex min-h-10 items-center gap-3">
				{canSelect ? (
					<SelectionCheckbox
						label={`Select ${filename}`}
						checked={entry.selected}
						disabled={isBusy}
						onChange={(selected) => onSelectionChange(entry.key, selected)}
					/>
				) : (
					<status.Icon
						aria-hidden="true"
						className={cn(
							"size-4 shrink-0",
							status.className,
							status.isActive && "motion-safe:animate-spin",
						)}
					/>
				)}
				<div className="min-w-0 flex-1">
					<span
						className={cn(
							"block text-sm",
							entry.state === "unresolved" ? "text-caption" : "text-heading",
							isExpanded ? "break-words" : "truncate",
						)}
						title={entry.preview?.file.title ?? filename}
					>
						{entry.preview?.file.title ?? filename}
					</span>
					{/* Sits in the row's second line, so an upload never changes its height. */}
					{entry.phase === "uploading" || entry.phase === "retry_wait" ? (
						<ImportTransferDetails entry={entry} />
					) : (
						<RowCaption
							entry={entry}
							albumArtists={albumArtists}
							isExpanded={isExpanded}
						/>
					)}
				</div>
				<span className="flex shrink-0 items-center gap-2">
					{isUploading ? (
						<span className="text-caption text-xs tabular-nums">
							{entry.progress}%
						</span>
					) : null}
					<span className={cn("text-xs", status.className)}>
						{status.label}
					</span>
					<Button
						type="button"
						variant="muted"
						size="icon-sm"
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
					className="absolute inset-x-0 bottom-0 h-0.5 rounded-none"
				/>
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
			{isProcessing ? null : (
				<>
					<DuplicateReview
						entry={entry}
						isBusy={isBusy}
						onDecisionChange={onDuplicateDecisionChange}
					/>
					<ImportErrors message={entry.errorMessage} />
				</>
			)}
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
		<Checkbox
			aria-label={label}
			checked={checked}
			disabled={disabled}
			onCheckedChange={(next) => onChange(next)}
		/>
	);
}

type ImportStatus = {
	label: string;
	className: string;
	Icon: LucideIcon;
	isActive?: boolean;
};

const doneStatus = (label: string): ImportStatus => ({
	label,
	className: "text-success",
	Icon: CircleCheck,
});

const failedStatus = (label: string): ImportStatus => ({
	label,
	className: "text-destructive",
	Icon: CircleX,
});

const stateStatuses: Record<
	Exclude<ImportFileEntry["state"], "unresolved">,
	ImportStatus
> = {
	accepted: doneStatus("Accepted"),
	rejected: failedStatus("Rejected"),
	completed: doneStatus("Completed"),
};

const outcomeStatuses: Record<
	NonNullable<ImportFileEntry["outcome"]>,
	ImportStatus
> = {
	imported: doneStatus("Imported"),
	replaced: doneStatus("Replaced"),
	rejected: failedStatus("Rejected"),
	failed: failedStatus("Failed"),
	not_attempted: {
		label: "Not attempted",
		className: "text-caption",
		Icon: CircleAlert,
	},
};

function importStatus(
	entry: ImportFileEntry,
	isConfirming: boolean,
): ImportStatus {
	if (isConfirming && entry.selected && !entry.outcome)
		return {
			label: "Importing…",
			className: "text-foreground",
			Icon: LoaderCircle,
			isActive: true,
		};
	if (entry.outcome) return outcomeStatuses[entry.outcome];
	if (entry.state !== "unresolved") return stateStatuses[entry.state];
	const label = importPhaseLabel(entry);
	return !entry.phase ||
		entry.phase === "queued" ||
		entry.phase === "retry_wait"
		? { label, className: "text-caption", Icon: Circle }
		: {
				label,
				className:
					entry.phase === "validating" ? "text-warning" : "text-foreground",
				Icon: LoaderCircle,
				isActive: true,
			};
}

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
			<RadioGroup
				aria-label="Track Replacement"
				value={entry.duplicateDecision ?? null}
				disabled={isBusy}
				onValueChange={(value) =>
					onDecisionChange(entry.key, value as DuplicateDecision)
				}
			>
				{duplicateDecisionOptions.map((option) => (
					<DuplicateDecisionOption key={option.value} option={option} />
				))}
			</RadioGroup>
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
	option,
}: {
	option: (typeof duplicateDecisionOptions)[number];
}) {
	return (
		<Label className="font-normal">
			<RadioGroupItem value={option.value} />
			{option.label}
		</Label>
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
		// An untagged list arrives as null (genres) or empty: both read alike.
		return Array.isArray(value) && value.length > 0
			? value.join(", ")
			: value == null || value === "" || Array.isArray(value)
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
