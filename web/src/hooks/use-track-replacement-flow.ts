import type {
	ManagedImportPreview,
	Track,
	TrackReplacementPreview,
} from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import {
	type DesktopImportSelection,
	desktopUploadImportFile,
	isDesktopClient,
	releaseDesktopImportSelections,
	selectDesktopImportFiles,
} from "#/desktop/bridge";
import { apiClient } from "#/lib/api";
import { invalidateLibraryCache } from "#/lib/invalidate-library-cache";
import { invalidatePlaylistCache } from "#/lib/playlist-query-cache";

export type TrackReplacementStep =
	| "select"
	| "uploading"
	| "review"
	| "replacing"
	| "completed";

type ReplacementUpload = {
	jobId: string;
	preview: ManagedImportPreview;
	replacement: TrackReplacementPreview;
};

export function useTrackReplacementFlow(onReplaced?: (track: Track) => void) {
	const playback = usePlayback();
	const queryClient = useQueryClient();
	const [track, setTrack] = useState<Track | null>(null);
	const [step, setStep] = useState<TrackReplacementStep>("select");
	const [upload, setUpload] = useState<ReplacementUpload | null>(null);
	const [progress, setProgress] = useState(0);
	const [error, setError] = useState<string | null>(null);
	const uploadController = useRef<AbortController | null>(null);
	const isBusy = step === "uploading" || step === "replacing";

	function reset() {
		setTrack(null);
		setStep("select");
		setUpload(null);
		setProgress(0);
		setError(null);
	}

	function open(selectedTrack: Track) {
		reset();
		setTrack(selectedTrack);
	}

	async function discardUpload(current: ReplacementUpload | null) {
		if (!current) return;
		await apiClient.cancelManagedImport(current.jobId).catch(() => undefined);
	}

	async function cancel() {
		if (isBusy) {
			uploadController.current?.abort();
			return;
		}
		const current = upload;
		reset();
		await discardUpload(current);
	}

	async function replaceWith(
		file: File | DesktopImportSelection,
	): Promise<void> {
		if (!track || isBusy) return;
		setStep("uploading");
		setError(null);
		setProgress(0);
		const controller = new AbortController();
		uploadController.current = controller;
		let jobId: string | null = null;
		try {
			const job = await apiClient.createTrackReplacement(track.id);
			jobId = job.id;
			const preview = await uploadReplacementFile(
				job.id,
				file,
				setProgress,
				controller.signal,
			);
			if (preview.status !== "awaiting_confirmation" || !preview.replacement) {
				throw new Error(
					preview.duplicateClassification === "exact_duplicate"
						? "The selected file already has the exact bytes of an existing Track."
						: "The replacement file was rejected.",
				);
			}
			setUpload({ jobId: job.id, preview, replacement: preview.replacement });
			setStep("review");
		} catch (cause) {
			if (jobId) await discardUpload({ jobId } as ReplacementUpload);
			if (controller.signal.aborted) {
				reset();
				return;
			}
			setError(replacementErrorMessage(cause));
			setStep("select");
		} finally {
			uploadController.current = null;
			if (!(file instanceof File)) {
				void releaseDesktopImportSelections([file.id]);
			}
		}
	}

	async function selectDesktopFile(): Promise<void> {
		const [selection, ...unused] = await selectDesktopImportFiles();
		if (unused.length > 0) {
			void releaseDesktopImportSelections(
				unused.map((extra) => extra.id),
			).catch((cause) => {
				console.error("Desktop import selection release failed", cause);
			});
		}
		if (!selection) return;
		await replaceWith(selection);
	}

	async function confirm(): Promise<void> {
		if (!track || !upload || isBusy) return;
		setStep("replacing");
		setError(null);
		try {
			await apiClient.confirmTrackReplacement(
				upload.jobId,
				upload.preview.revision,
				upload.replacement.confirmationToken,
			);
		} catch (cause) {
			setError(replacementErrorMessage(cause));
			setStep("review");
			return;
		}
		// The server has committed the replacement; nothing below may report it as failed.
		if (playback.currentTrack?.id === track.id) {
			playback.stopPlayback();
		}
		setStep("completed");
		onReplaced?.(track);
		try {
			await Promise.all([
				invalidateLibraryCache(queryClient),
				invalidatePlaylistCache(queryClient),
				playback.refreshQueue(),
			]);
		} catch (cause) {
			console.error("Refresh after Track Replacement failed", cause);
		}
	}

	return {
		track,
		step,
		preview: upload?.replacement ?? null,
		progress,
		error,
		isBusy,
		isDesktop: isDesktopClient(),
		open,
		cancel,
		replaceWith,
		selectDesktopFile,
		confirm,
		close: reset,
	};
}

async function uploadReplacementFile(
	jobId: string,
	file: File | DesktopImportSelection,
	onProgress: (progress: number) => void,
	signal: AbortSignal,
): Promise<ManagedImportPreview> {
	if (file instanceof File) {
		return apiClient.uploadManagedImportFile(
			jobId,
			file.name,
			file,
			onProgress,
			signal,
		);
	}
	const response = await desktopUploadImportFile(
		file.id,
		jobId,
		onProgress,
		signal,
	);
	const body = (await response.json()) as
		| ManagedImportPreview
		| { message?: string };
	if (!response.ok) {
		throw new Error(
			"message" in body && typeof body.message === "string"
				? body.message
				: `HTTP ${response.status}`,
		);
	}
	return body as ManagedImportPreview;
}

function replacementErrorMessage(error: unknown): string {
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
	return "Track Replacement failed. Please try again.";
}
