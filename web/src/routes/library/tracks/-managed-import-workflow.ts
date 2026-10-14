import {
	ApiError,
	type ManagedImportAlbumDecision,
	type ManagedImportBatch,
	type ManagedImportBatchFile,
	type ManagedImportDuplicateDecision,
	type ManagedImportPreview,
} from "@repo/api-client";
import { useCallback, useRef, useState } from "react";
import {
	type DesktopImportSelection,
	desktopUploadImportFile,
	releaseDesktopImportSelections,
	selectDesktopImportFiles,
	selectDesktopImportFolder,
} from "#/desktop/bridge";
import { apiClient } from "#/lib/api";
import {
	importPositionKey,
	importReviewProblems,
	reviewImportEntries,
} from "./-import-review";

import { formatImportIssue, formatImportIssues } from "./-import-validation";

type ImportState = "idle" | "uploading" | "confirming";

import { runImportUploads, waitForImport } from "./-import-upload-queue";

const IMPORT_POLL_INTERVAL_MS = 1000;

import { useImportSessionLifecycle } from "./-import-session-lifecycle";

const SUPPORTED_AUDIO_EXTENSIONS = [
	"flac",
	"mp3",
	"m4a",
	"ogg",
	"opus",
	"wav",
] as const;
const SUPPORTED_AUDIO_EXTENSION_SET = new Set<string>(
	SUPPORTED_AUDIO_EXTENSIONS,
);
export const SUPPORTED_AUDIO_FILE_ACCEPT = SUPPORTED_AUDIO_EXTENSIONS.map(
	(extension) => `.${extension}`,
).join(",");

export type ImportFileEntry = {
	key: string;
	file: File | DesktopImportSelection;
	jobId?: string;
	progress: number;
	phase?: ManagedImportBatchFile["phase"] | "retry_wait";
	transferredBytes?: number;
	retryCount?: number;
	retryAt?: number;
	canRetry?: boolean;
	startedAt?: number;
	state: "accepted" | "rejected" | "unresolved" | "completed";
	selected: boolean;
	hasSelectionOverride: boolean;
	preview?: ManagedImportPreview;
	errorMessage?: string;
	outcome?: ManagedImportBatchFile["outcome"];
	duplicateDecision?: DuplicateDecision;
};

export type DuplicateDecision = ManagedImportDuplicateDecision["action"];

function requiresDuplicateDecision(
	preview: ManagedImportPreview | undefined,
): boolean {
	return Boolean(preview?.matchingTracks?.length);
}

type ImportEntrySummary = {
	total: number;
	processed: number;
	unresolved: number;
	accepted: number;
	/** Track Replacement candidates still waiting for a decision. */
	needsReview: number;
	rejected: number;
	completed: number;
	/** Whole-batch percentage; unresolved rows never count as finished. */
	percent: number;
};

export function summarizeImportEntries(
	entries: ImportFileEntry[],
): ImportEntrySummary {
	const summary: ImportEntrySummary = {
		total: entries.length,
		processed: 0,
		unresolved: 0,
		accepted: 0,
		needsReview: 0,
		rejected: 0,
		completed: 0,
		percent: 0,
	};
	let progressTotal = 0;
	for (const entry of entries) {
		if (entry.state === "unresolved") {
			summary.unresolved += 1;
			progressTotal += Math.max(0, Math.min(99, entry.progress));
			continue;
		}
		summary.processed += 1;
		progressTotal += 100;
		if (entry.state === "rejected") summary.rejected += 1;
		else if (entry.state === "completed") summary.completed += 1;
		else if (
			requiresDuplicateDecision(entry.preview) &&
			!entry.duplicateDecision
		)
			summary.needsReview += 1;
		else summary.accepted += 1;
	}
	summary.percent =
		entries.length === 0 ? 0 : Math.round(progressTotal / entries.length);
	return summary;
}

export function useManagedImportWorkflow({
	onOpenChange,
	onCommitted,
}: {
	onOpenChange: (isOpen: boolean) => void;
	onCommitted: () => Promise<void>;
}) {
	const state = useImportWorkflowState();
	useImportSessionActivity(state, onCommitted);
	const isBusy = state.importState !== "idle" || state.artworkUploads > 0;
	const reviewEntries = reviewImportEntries(
		state.entries,
		state.batch,
		state.albumDecisions,
	);
	const reviewProblems = importReviewProblems(
		reviewEntries,
		state.batch?.albums ?? [],
		state.albumDecisions,
	);
	const isCompleted = state.batch?.status === "completed";
	const isConfirming =
		state.importState === "confirming" || state.batch?.status === "confirming";
	const isCloseLocked =
		(isBusy && !state.batch) ||
		state.importState === "confirming" ||
		state.batch?.status === "confirming";
	const canConfirm = Boolean(
		state.batch &&
		state.batch.status !== "confirming" &&
		!isBusy &&
		!isCompleted &&
		state.entries.some(
			(entry) => entry.state === "accepted" && entry.selected,
		) &&
		reviewProblems.length === 0,
	);

	const { updateEntry } = state;
	const handleSelectionChange = useCallback(
		(key: string, selected: boolean) => {
			state.setEntries((current) => {
				const chosen = current.find((entry) => entry.key === key);
				return current.map((entry) =>
					entry.key === key
						? { ...entry, selected, hasSelectionOverride: true }
						: selected &&
							  chosen &&
							  importPositionKey(entry) === importPositionKey(chosen)
							? { ...entry, selected: false, hasSelectionOverride: true }
							: entry,
				);
			});
		},
		[state.setEntries],
	);
	const handleDuplicateDecisionChange = useCallback(
		(key: string, duplicateDecision: DuplicateDecision) =>
			updateEntry(key, {
				duplicateDecision,
				selected: duplicateDecision !== "do_not_import",
				hasSelectionOverride: true,
			}),
		[updateEntry],
	);
	return {
		importState: state.importState,
		entries: reviewEntries,
		batchId: state.batch?.id,
		albums: state.batch?.albums ?? [],
		albumDecisions: state.albumDecisions,
		reviewProblems,
		handleAlbumDecision: (decision: ManagedImportAlbumDecision) => {
			state.setAlbumDecisions((current) => ({
				...current,
				[decision.albumKey]: decision,
			}));
			state.setEntries((current) =>
				current.map((entry) => ({ ...entry, duplicateDecision: undefined })),
			);
		},
		handleArtworkUploadState: (isUploading: boolean) =>
			state.setArtworkUploads((current) =>
				Math.max(0, current + (isUploading ? 1 : -1)),
			),
		handleArtworkUploaded: async () => {
			if (state.batch)
				state.setBatch(await apiClient.getManagedImportBatch(state.batch.id));
		},
		errorMessage: state.errorMessage,
		isBusy,
		isCloseLocked,
		isPickerLocked: isBusy || state.entries.length > 0 || Boolean(state.batch),
		isSelectionLocked: isBusy || state.batch?.status === "confirming",
		isCompleted,
		isConfirming,
		canConfirm,
		handleFiles: createFileHandler(state),
		handleDesktopSelection: createDesktopSelectionHandler(state),
		handleConfirm: createConfirmHandler(state, canConfirm, onCommitted),
		handleRetry: createRetryHandler(state),
		canRetry:
			!isBusy && !isCompleted && state.entries.some((entry) => entry.canRetry),
		handleCancel: createCancelHandler(state, onOpenChange),
		handleSelectionChange,
		handleDuplicateDecisionChange,
		handleOpenChange: createOpenHandler(state, isCloseLocked, onOpenChange),
	};
}

function useImportWorkflowState() {
	const [importState, setImportState] = useState<ImportState>("idle");
	const [batch, setBatch] = useState<ManagedImportBatch>();
	const [artworkUploads, setArtworkUploads] = useState(0);
	const [albumDecisions, setAlbumDecisions] = useState<
		Record<string, ManagedImportAlbumDecision>
	>({});
	const [entries, setEntries] = useState<ImportFileEntry[]>([]);
	const [errorMessage, setErrorMessage] = useState("");
	const activeUploadController = useRef<AbortController | undefined>(undefined);
	const isDesktopSelectionPending = useRef(false);
	// Stable identity lets memoized rows skip re-rendering when a sibling's
	// upload progress changes.
	const updateEntry = useCallback(
		(key: string, patch: Partial<ImportFileEntry>) => {
			setEntries((current) =>
				current.map((entry) =>
					entry.key === key ? { ...entry, ...patch } : entry,
				),
			);
		},
		[],
	);
	function reset() {
		setBatch(undefined);
		setAlbumDecisions({});
		setEntries([]);
		setErrorMessage("");
	}
	return {
		importState,
		setImportState,
		batch,
		setBatch,
		albumDecisions,
		setAlbumDecisions,
		artworkUploads,
		setArtworkUploads,
		entries,
		setEntries,
		errorMessage,
		setErrorMessage,
		activeUploadController,
		isDesktopSelectionPending,
		updateEntry,
		reset,
	};
}

type WorkflowState = ReturnType<typeof useImportWorkflowState>;

function createFileHandler(state: WorkflowState) {
	return async (fileList: FileList | Array<File | DesktopImportSelection>) => {
		if (state.entries.length > 0 || state.batch) return;
		const files = Array.from(fileList).filter(isSupportedVisibleAudioFile);
		if (files.length === 0) return;
		state.setImportState("uploading");
		state.setErrorMessage("");
		const initialEntries = files.map(createImportFileEntry);
		state.setEntries(initialEntries);
		const uploadController = new AbortController();
		state.activeUploadController.current = uploadController;
		try {
			const createdBatch = await apiClient.createManagedImportBatch();
			if (uploadController.signal.aborted) {
				await apiClient.cancelManagedImportBatch(createdBatch.id);
				return;
			}
			state.setBatch(createdBatch);
			const { batch: previewBatch, entries: uploadedEntries } =
				await uploadImportBatch(
					createdBatch.id,
					initialEntries,
					state.updateEntry,
					uploadController.signal,
				);
			uploadController.signal.throwIfAborted();
			state.setBatch(previewBatch);
			state.setEntries((current) =>
				mergeBatchFiles(
					copyJobAssignments(current, uploadedEntries),
					previewBatch.files,
				),
			);
		} catch (error) {
			if (uploadController.signal.aborted) return;
			state.setErrorMessage(importErrorMessage(error));
		} finally {
			if (state.activeUploadController.current === uploadController) {
				state.activeUploadController.current = undefined;
			}
			state.setImportState("idle");
		}
	};
}

function createDesktopSelectionHandler(state: WorkflowState) {
	const handleFiles = createFileHandler(state);
	return async (isDirectory: boolean) => {
		if (
			state.isDesktopSelectionPending.current ||
			state.entries.length > 0 ||
			state.batch
		) {
			return;
		}
		state.isDesktopSelectionPending.current = true;
		state.setImportState("uploading");
		try {
			const files = await (isDirectory
				? selectDesktopImportFolder()
				: selectDesktopImportFiles());
			state.isDesktopSelectionPending.current = false;
			await handleFiles(files);
		} catch (error) {
			state.setErrorMessage(importErrorMessage(error));
		} finally {
			state.isDesktopSelectionPending.current = false;
			state.setImportState("idle");
		}
	};
}

function isSupportedVisibleAudioFile(
	file: File | DesktopImportSelection,
): boolean {
	if (isDesktopImportSelection(file)) return true;
	const clientPath = file.webkitRelativePath || file.name;
	if (clientPath.split("/").some((segment) => segment.startsWith("."))) {
		return false;
	}
	const extensionSeparator = file.name.lastIndexOf(".");
	if (extensionSeparator < 0) return false;
	return SUPPORTED_AUDIO_EXTENSION_SET.has(
		file.name.slice(extensionSeparator + 1).toLowerCase(),
	);
}

function createConfirmHandler(
	state: WorkflowState,
	canConfirm: boolean,
	onCommitted: () => Promise<void>,
) {
	return async () => {
		if (!state.batch || !canConfirm) return;
		state.setImportState("confirming");
		state.setErrorMessage("");
		try {
			const currentBatch = await prepareConfirmationBatch(
				state.batch.id,
				state.entries,
			);
			let reconciledEntries = attachServerJobs(
				state.entries,
				currentBatch.files,
			);

			const changedReplacement = state.entries.some((entry) => {
				if (entry.duplicateDecision !== "replace_existing") return false;
				const previous = entry.preview?.matchingTracks?.[0];
				const file = currentBatch.files.find(
					(file) => file.jobId === entry.jobId,
				);
				const album = currentBatch.albums?.find(
					(album) => album.key === entry.preview?.file.albumKey,
				);
				const current =
					album?.existingAlbums
						.flatMap((album) => album.tracks)
						.find((track) => track.trackId === previous?.trackId) ??
					file?.preview?.matchingTracks?.[0];
				return (
					current &&
					previous &&
					(current.trackId !== previous.trackId ||
						current.revision !== previous.revision)
				);
			});
			state.setBatch(currentBatch);
			reconciledEntries = mergeBatchFiles(
				reconciledEntries,
				currentBatch.files,
			);
			reconciledEntries = reviewImportEntries(
				reconciledEntries,
				currentBatch,
				state.albumDecisions,
			);
			state.setEntries(reconciledEntries);

			if (changedReplacement) {
				state.setEntries(
					reconciledEntries.map((entry) => ({
						...entry,
						duplicateDecision: undefined,
					})),
				);
				state.setErrorMessage(
					"The existing Track changed. Review its changes and confirm replacement again.",
				);
				return;
			}
			const problems = importReviewProblems(
				reconciledEntries,
				currentBatch.albums ?? [],
				state.albumDecisions,
			);
			if (
				state.entries.some(
					(entry) =>
						entry.selected &&
						!reconciledEntries.find((current) => current.key === entry.key)
							?.selected,
				)
			) {
				problems.push(
					"Import preview changed. Review the selected files before confirming again.",
				);
			}
			if (problems.length) {
				state.setErrorMessage(problems.join(" "));
				return;
			}
			const report = await confirmImportBatch(
				currentBatch,
				reconciledEntries,
				Object.values(state.albumDecisions),
			);
			state.setBatch(report);
			state.setEntries((current) => mergeBatchFiles(current, report.files));
			await releaseNativeSelections(reconciledEntries);
			if (hasLibraryMutation(report)) await onCommitted();
		} catch (error) {
			await handleConfirmationFailure(state, error, onCommitted);
		} finally {
			state.setImportState("idle");
		}
	};
}

async function prepareConfirmationBatch(
	batchId: string,
	entries: ImportFileEntry[],
) {
	const batch = await apiClient.getManagedImportBatch(batchId);
	let hasCanceledJobs = false;
	for (const file of batch.files) {
		const entry = entries.find((candidate) => candidate.jobId === file.jobId);
		if (
			file.state !== "unresolved" ||
			file.phase !== "queued" ||
			entry?.state !== "rejected"
		)
			continue;
		await apiClient.cancelManagedImport(file.jobId);
		hasCanceledJobs = true;
	}
	return hasCanceledJobs ? apiClient.getManagedImportBatch(batchId) : batch;
}

async function handleConfirmationFailure(
	state: WorkflowState,
	error: unknown,
	onCommitted: () => Promise<void>,
) {
	if (!state.batch) return;
	try {
		const batch = await apiClient.getManagedImportBatch(state.batch.id);
		state.setBatch(batch);
		state.setEntries((entries) => mergeBatchFiles(entries, batch.files));
		if (batch.status === "completed") {
			await releaseNativeSelections(state.entries);
			if (hasLibraryMutation(batch)) await onCommitted();
			state.setErrorMessage("");
			return;
		}
		state.setErrorMessage(
			error instanceof ApiError &&
				error.body.code === "import_revision_conflict"
				? "Import preview changed. Review the Album and Track choices."
				: importErrorMessage(error),
		);
	} catch (refreshError) {
		state.setErrorMessage(
			`${importErrorMessage(error)} Refresh failed: ${importErrorMessage(refreshError)}`,
		);
	}
}

function createOpenHandler(
	state: WorkflowState,
	isBusy: boolean,
	onOpenChange: (isOpen: boolean) => void,
) {
	return async (nextIsOpen: boolean) => {
		if (state.isDesktopSelectionPending.current) return;
		if (!nextIsOpen && (state.importState !== "idle" || isBusy)) {
			onOpenChange(false);
			return;
		}
		if (nextIsOpen) {
			onOpenChange(true);
			return;
		}
		await createCancelHandler(state, onOpenChange)();
	};
}

async function uploadImportBatch(
	batchId: string,
	entries: ImportFileEntry[],
	updateEntry: (key: string, patch: Partial<ImportFileEntry>) => void,
	signal: AbortSignal,
): Promise<{ batch: ManagedImportBatch; entries: ImportFileEntry[] }> {
	const preparedEntries = await createBatchJobs(
		batchId,
		entries,
		updateEntry,
		signal,
	);
	await uploadPreparedEntries(batchId, preparedEntries, updateEntry, signal);
	signal.throwIfAborted();
	const batch = await apiClient.getManagedImportBatch(batchId);
	return {
		batch,
		entries: attachServerJobs(
			attachCreatedJobs(entries, preparedEntries),
			batch.files,
		),
	};
}

async function createBatchJobs(
	batchId: string,
	entries: ImportFileEntry[],
	updateEntry: (key: string, patch: Partial<ImportFileEntry>) => void,
	signal: AbortSignal,
) {
	const preparedEntries: Array<{ entry: ImportFileEntry; jobId: string }> = [];
	for (const entry of entries) {
		signal.throwIfAborted();
		try {
			const job = await apiClient.createManagedImportJob(batchId, entry.key);
			updateEntry(entry.key, { jobId: job.id });
			preparedEntries.push({ entry, jobId: job.id });
		} catch (error) {
			updateEntry(entry.key, {
				state: "rejected",
				errorMessage: importErrorMessage(error),
			});
		}
	}
	if (preparedEntries.length < entries.length) {
		const batch = await apiClient.getManagedImportBatch(batchId);
		signal.throwIfAborted();
		for (const entry of entries) {
			if (preparedEntries.some((item) => item.entry.key === entry.key))
				continue;
			const recovered = batch.files.find(
				(file) =>
					file.clientFileId === entry.key && file.state === "unresolved",
			);
			if (!recovered) continue;
			updateEntry(entry.key, {
				jobId: recovered.jobId,
				state: "unresolved",
				phase: "queued",
				errorMessage: undefined,
			});
			preparedEntries.push({ entry, jobId: recovered.jobId });
		}
	}
	return preparedEntries;
}

async function uploadFile(
	jobId: string,
	entry: ImportFileEntry,
	updateEntry: (key: string, patch: Partial<ImportFileEntry>) => void,
	signal?: AbortSignal,
) {
	try {
		// Transports fire many progress events per second; only publish a state
		// update when the rounded percentage actually changes so the dialog does
		// not re-render on every network chunk.
		updateEntry(entry.key, {
			state: "unresolved",
			phase: "uploading",
			progress: 0,
			transferredBytes: 0,
			retryAt: undefined,
			errorMessage: undefined,
			startedAt: Date.now(),
		});
		let lastProgress = -1;
		const onProgress = (progress: number, transferredBytes?: number) => {
			if (progress === lastProgress) return;
			lastProgress = progress;
			updateEntry(entry.key, {
				progress,
				transferredBytes:
					transferredBytes ?? Math.round((entry.file.size * progress) / 100),
				phase: progress >= 100 ? "validating" : "uploading",
			});
		};
		const preview = isDesktopImportSelection(entry.file)
			? await uploadDesktopFile(entry.file, jobId, onProgress, signal)
			: await apiClient.uploadManagedImportFile(
					jobId,
					entry.file.name,
					entry.file,
					onProgress,
					signal,
				);
		signal?.throwIfAborted();
		const duplicateClassification = preview.duplicateClassification ?? "none";
		updateEntry(entry.key, {
			state:
				duplicateClassification === "exact_duplicate" ? "rejected" : "accepted",
			selected: duplicateClassification === "none",
			preview,
			progress: 100,
			phase: duplicateClassification === "exact_duplicate" ? "failed" : "ready",
			canRetry: false,
		});
		return false;
	} catch (error) {
		if (signal?.aborted) throw error;
		const canRetry = isRetryableTransferError(error);
		updateEntry(entry.key, {
			state: "rejected",
			phase: "failed",
			canRetry,
			selected: false,
			errorMessage: importErrorMessage(error),
		});
		return canRetry;
	}
}

async function uploadDesktopFile(
	file: DesktopImportSelection,
	jobId: string,
	onProgress: (progress: number, transferredBytes?: number) => void,
	signal?: AbortSignal,
): Promise<ManagedImportPreview> {
	const response = await desktopUploadImportFile(
		file.id,
		jobId,
		onProgress,
		signal,
	);
	const body = await response.json();
	if (!response.ok) throw new ApiError(response.status, body);
	return body as ManagedImportPreview;
}

function confirmImportBatch(
	batch: ManagedImportBatch,
	entries: ImportFileEntry[],
	albumDecisions: ManagedImportAlbumDecision[],
) {
	const selected = entries.filter(
		(entry): entry is ImportFileEntry & { jobId: string } =>
			entry.state === "accepted" && entry.selected && Boolean(entry.jobId),
	);
	const decisions = selected.flatMap((entry) =>
		entry.duplicateDecision === "replace_existing" &&
		entry.preview?.matchingTracks?.length
			? [
					{
						jobId: entry.jobId,
						action: entry.duplicateDecision,
						trackId: entry.preview.matchingTracks[0]?.trackId,
						targetRevision: entry.preview.matchingTracks[0]?.revision,
					} satisfies ManagedImportDuplicateDecision,
				]
			: [],
	);
	return apiClient.confirmManagedImportBatch(
		batch.id,
		batch.revision,
		selected.map((entry) => entry.jobId),
		decisions.length ? decisions : undefined,
		albumDecisions.length ? albumDecisions : undefined,
	);
}

function hasLibraryMutation(batch: ManagedImportBatch): boolean {
	return batch.files.some(
		(file) => file.outcome === "imported" || file.outcome === "replaced",
	);
}

function createImportFileEntry(
	file: File | DesktopImportSelection,
): ImportFileEntry {
	return {
		key: crypto.randomUUID(),
		file,
		progress: 0,
		phase: "queued",
		state: "unresolved",
		selected: false,
		hasSelectionOverride: false,
	};
}

function isDesktopImportSelection(
	file: File | DesktopImportSelection,
): file is DesktopImportSelection {
	return !(file instanceof File);
}

function releaseNativeSelections(entries: ImportFileEntry[]): Promise<void> {
	const selectionIds = entries.flatMap((entry) =>
		isDesktopImportSelection(entry.file) ? [entry.file.id] : [],
	);
	return selectionIds.length > 0
		? releaseDesktopImportSelections(selectionIds)
		: Promise.resolve();
}

function mergeBatchFiles(
	entries: ImportFileEntry[],
	files: ManagedImportBatchFile[],
): ImportFileEntry[] {
	return entries.map((entry) => {
		const result = files.find((file) => file.jobId === entry.jobId);
		if (!result) return entry;
		const serverError =
			formatImportIssues(result.issues) ??
			(result.errorCode === "missing_artwork"
				? formatImportIssue({
						code: result.errorCode,
						field: "artwork",
						reason: result.errorReason ?? "",
					})
				: (result.errorReason ?? result.errorCode));
		return {
			...entry,
			state:
				result.state === "unresolved" && entry.phase === "failed"
					? "rejected"
					: result.state,
			phase:
				result.state === "unresolved"
					? entry.phase
					: (result.phase ??
						(result.state === "accepted" ? "ready" : "completed")),
			canRetry: result.state === "unresolved" ? entry.canRetry : false,
			selected:
				result.state === "accepted" && entry.hasSelectionOverride
					? entry.selected
					: result.selected,
			preview: result.preview ?? entry.preview,
			progress: entry.progress,
			errorMessage:
				result.state === "accepted" || result.state === "completed"
					? serverError
					: (serverError ?? entry.errorMessage),
			outcome: result.outcome,
		};
	});
}

function attachCreatedJobs(
	entries: ImportFileEntry[],
	preparedEntries: Array<{ entry: ImportFileEntry; jobId: string }>,
): ImportFileEntry[] {
	return entries.map((entry) => {
		const prepared = preparedEntries.find(
			(item) => item.entry.key === entry.key,
		);
		return prepared ? { ...entry, jobId: prepared.jobId } : entry;
	});
}

function copyJobAssignments(
	entries: ImportFileEntry[],
	assignedEntries: ImportFileEntry[],
): ImportFileEntry[] {
	return entries.map((entry) => {
		const assigned = assignedEntries.find((item) => item.key === entry.key);
		return assigned?.jobId ? { ...entry, jobId: assigned.jobId } : entry;
	});
}

function attachServerJobs(
	entries: ImportFileEntry[],
	files: ManagedImportBatchFile[],
): ImportFileEntry[] {
	return entries.map((entry) => {
		if (entry.jobId) return entry;
		const file = files.find(
			(candidate) =>
				candidate.state === "unresolved" &&
				candidate.clientFileId === entry.key,
		);
		return file
			? {
					...entry,
					jobId: file.jobId,
					state: "unresolved",
					errorMessage: undefined,
				}
			: entry;
	});
}

function hasUncommittedImportWork(state: WorkflowState): boolean {
	return (
		state.entries.some(
			(entry) => entry.state === "accepted" || entry.state === "unresolved",
		) ||
		(state.batch?.files.some(
			(file) => file.state === "accepted" || file.state === "unresolved",
		) ??
			false)
	);
}

function isImportAlreadyGone(error: unknown): boolean {
	return (
		error instanceof ApiError &&
		(error.status === 404 || error.body.code === "import_not_found")
	);
}

function importErrorMessage(error: unknown): string {
	if (error instanceof ApiError) {
		const message = formatImportIssues(error.body.issues);
		if (message) return message;
		if (error.body.code === "missing_artwork")
			return formatImportIssue({
				code: error.body.code,
				field: "artwork",
				reason: error.message,
			});
	}
	if (error instanceof Error && error.message.trim()) return error.message;
	if (
		typeof error === "object" &&
		error !== null &&
		"message" in error &&
		typeof error.message === "string" &&
		error.message.trim()
	) {
		return error.message;
	}
	return "Managed Import failed. Please try again.";
}

function isRetryableTransferError(error: unknown): boolean {
	if (error instanceof ApiError) {
		return (
			error.body.code === "upload_interrupted" ||
			[408, 429, 502, 503, 504].includes(error.status)
		);
	}
	if (typeof error === "object" && error !== null && "code" in error) {
		return error.code === "transport_error";
	}
	return !(error instanceof DOMException && error.name === "AbortError");
}

type PreparedUpload = { entry: ImportFileEntry; jobId: string };

async function uploadPreparedEntries(
	batchId: string,
	prepared: PreparedUpload[],
	updateEntry: (key: string, patch: Partial<ImportFileEntry>) => void,
	signal?: AbortSignal,
) {
	await runImportUploads(
		prepared,
		async ({ entry, jobId }, isRetry) => {
			if (isRetry) {
				try {
					if (
						await reconcileBeforeRetry(
							batchId,
							entry,
							jobId,
							updateEntry,
							signal,
						)
					)
						return false;
				} catch (error) {
					signal?.throwIfAborted();
					updateEntry(entry.key, {
						phase: "failed",
						state: "rejected",
						canRetry: isRetryableTransferError(error),
						retryAt: undefined,
						errorMessage: importErrorMessage(error),
					});
					return isRetryableTransferError(error);
				}
			}
			const shouldRetry = await uploadFile(jobId, entry, updateEntry, signal);
			if (!shouldRetry) return false;
			try {
				return !(await reconcileBeforeRetry(
					batchId,
					entry,
					jobId,
					updateEntry,
					signal,
				));
			} catch (error) {
				signal?.throwIfAborted();
				updateEntry(entry.key, {
					errorMessage: importErrorMessage(error),
					canRetry: isRetryableTransferError(error),
				});
				return isRetryableTransferError(error);
			}
		},
		({ entry }, retryCount, retryAt) => {
			updateEntry(entry.key, {
				state: "unresolved",
				phase: "retry_wait",
				retryCount,
				retryAt,
			});
		},
		signal,
	);
}

async function reconcileBeforeRetry(
	batchId: string,
	entry: ImportFileEntry,
	jobId: string,
	updateEntry: (key: string, patch: Partial<ImportFileEntry>) => void,
	signal?: AbortSignal,
): Promise<boolean> {
	while (true) {
		signal?.throwIfAborted();
		const batch = await apiClient.getManagedImportBatch(batchId);
		signal?.throwIfAborted();
		const file = batch.files.find((candidate) => candidate.jobId === jobId);
		if (!file)
			throw new Error(
				"Import file is no longer available. Start a new import.",
			);
		if (file.state !== "unresolved") {
			const merged = mergeBatchFiles([{ ...entry, jobId }], [file])[0];
			if (merged) updateEntry(entry.key, merged);
			return true;
		}
		if (!file.phase || file.phase === "queued" || file.phase === "failed")
			return false;
		updateEntry(entry.key, { phase: file.phase, retryAt: undefined });
		await waitForImport(IMPORT_POLL_INTERVAL_MS, signal);
	}
}

function createRetryHandler(state: WorkflowState) {
	return async () => {
		if (!state.batch || state.importState !== "idle") return;
		const controller = new AbortController();
		state.activeUploadController.current = controller;
		state.setImportState("uploading");
		state.setErrorMessage("");
		try {
			const prepared: PreparedUpload[] = [];
			for (const entry of state.entries) {
				if (!entry.canRetry || !entry.jobId) continue;
				state.updateEntry(entry.key, {
					retryCount: 0,
					retryAt: undefined,
					phase: "queued",
				});
				if (
					!(await reconcileBeforeRetry(
						state.batch.id,
						entry,
						entry.jobId,
						state.updateEntry,
						controller.signal,
					))
				) {
					prepared.push({ entry, jobId: entry.jobId });
				}
			}
			await uploadPreparedEntries(
				state.batch.id,
				prepared,
				state.updateEntry,
				controller.signal,
			);
			controller.signal.throwIfAborted();
			const batch = await apiClient.getManagedImportBatch(state.batch.id);
			controller.signal.throwIfAborted();
			state.setBatch(batch);
			state.setEntries((entries) => mergeBatchFiles(entries, batch.files));
		} catch (error) {
			if (!controller.signal.aborted)
				state.setErrorMessage(importErrorMessage(error));
		} finally {
			if (state.activeUploadController.current === controller)
				state.activeUploadController.current = undefined;
			state.setImportState("idle");
		}
	};
}

function createCancelHandler(
	state: WorkflowState,
	onOpenChange: (isOpen: boolean) => void,
) {
	return async () => {
		if (
			state.importState === "confirming" ||
			state.batch?.status === "confirming"
		)
			return;
		if (
			state.batch &&
			hasUncommittedImportWork(state) &&
			!window.confirm("Cancel this import and remove all uncommitted uploads?")
		)
			return;
		state.activeUploadController.current?.abort();
		try {
			if (state.batch && state.batch.status !== "completed") {
				try {
					await apiClient.cancelManagedImportBatch(state.batch.id);
				} catch (error) {
					if (!isImportAlreadyGone(error)) throw error;
				}
			}
			await releaseNativeSelections(state.entries);
			state.reset();
			onOpenChange(false);
		} catch (error) {
			state.setErrorMessage(importErrorMessage(error));
		}
	};
}

function useImportSessionActivity(
	state: WorkflowState,
	onCommitted: () => Promise<void>,
) {
	useImportSessionLifecycle({
		batch: state.batch,
		isProcessing: state.importState !== "idle",
		onBatch: (batch) => handleLiveBatch(state, batch, onCommitted),
		onError: (error) =>
			state.setErrorMessage(
				`Import status unavailable: ${importErrorMessage(error)}`,
			),
		onExit: () => {
			state.activeUploadController.current?.abort();
			if (state.batch?.status === "uploading") {
				void apiClient
					.cancelManagedImportBatch(state.batch.id, true)
					.catch((error) => console.error("Import exit cleanup failed", error));
			}
		},
	});
}

async function handleLiveBatch(
	state: WorkflowState,
	batch: ManagedImportBatch,
	onCommitted: () => Promise<void>,
) {
	applyLiveProgress(state, batch);
	state.setErrorMessage((message) =>
		message.startsWith("Import status unavailable:") ? "" : message,
	);
	if (batch.status !== "completed" || state.importState !== "idle") return;
	state.setBatch(batch);
	await releaseNativeSelections(state.entries);
	if (hasLibraryMutation(batch)) await onCommitted();
}

function applyLiveProgress(state: WorkflowState, batch: ManagedImportBatch) {
	if (
		state.importState === "confirming" ||
		state.batch?.status === "confirming"
	) {
		state.setEntries((entries) => mergeBatchFiles(entries, batch.files));
		return;
	}
	state.setEntries((entries) =>
		entries.map((entry) => {
			const file = batch.files.find(
				(candidate) => candidate.jobId === entry.jobId,
			);
			if (
				!file?.phase ||
				!["validating"].includes(file.phase) ||
				entry.phase === "failed"
			)
				return entry;
			return {
				...entry,
				phase: file.phase,
				transferredBytes: file.transferredBytes ?? entry.transferredBytes,
			};
		}),
	);
}
