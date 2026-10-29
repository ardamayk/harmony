import {
	ApiError,
	SOULSEEK_ACQUISITION_CAPABILITY,
	type SoulseekAcquisition,
	type SoulseekAcquisitionFile,
	type SoulseekAcquisitionList,
} from "@repo/api-client";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "#/components/ui/button";
import { useServerCapability } from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";
import {
	formatAcquisitionQuality,
	isActiveAcquisition,
	soulseekErrorMessage,
	soulseekQueryKeys,
} from "#/lib/soulseek";
import { ProgressBar } from "./-import-progress";
import { CANCEL_ACQUISITION_PROMPT } from "./-managed-import-workflow";

const ACQUISITION_POLL_INTERVAL_MS = 2000;

/** Finished and cancelled acquisitions leave the list; their Tracks are in the library. */
function isShown(acquisition: SoulseekAcquisition): boolean {
	return acquisition.state !== "completed" && acquisition.state !== "cancelled";
}

/**
 * Soulseek Acquisitions beside the listener's own Import Batches: downloads
 * in progress, imports, reviews waiting for confirmation and failures. The
 * list polls while any acquisition is still moving.
 */
export function SoulseekAcquisitions({
	isBusy,
	onReview,
}: {
	isBusy: boolean;
	onReview: (acquisitionId: string, batchId: string) => void;
}) {
	const canAcquire = useServerCapability(SOULSEEK_ACQUISITION_CAPABILITY);
	const acquisitions = useQuery({
		queryKey: soulseekQueryKeys.acquisitions,
		queryFn: () => apiClient.listSoulseekAcquisitions(),
		enabled: canAcquire,
		refetchInterval: (query) =>
			query.state.data?.items.some(isActiveAcquisition)
				? ACQUISITION_POLL_INTERVAL_MS
				: false,
	});
	if (!canAcquire) return null;
	const items = acquisitions.data?.items.filter(isShown) ?? [];
	if (acquisitions.isError && items.length === 0)
		return (
			<p role="alert" className="text-destructive text-sm">
				Soulseek Acquisitions could not be loaded.{" "}
				<button
					type="button"
					className="underline"
					onClick={() => void acquisitions.refetch()}
				>
					Retry
				</button>
			</p>
		);
	if (items.length === 0) return null;
	return (
		<section aria-label="Soulseek Acquisitions" className="grid gap-2">
			<h3 className="font-medium text-sm">From Soulseek</h3>
			<ul className="overflow-hidden rounded-md border border-border">
				{items.map((acquisition) => (
					<AcquisitionRow
						key={acquisition.acquisitionId}
						acquisition={acquisition}
						isBusy={isBusy}
						onReview={onReview}
					/>
				))}
			</ul>
		</section>
	);
}

function AcquisitionRow({
	acquisition,
	isBusy,
	onReview,
}: {
	acquisition: SoulseekAcquisition;
	isBusy: boolean;
	onReview: (acquisitionId: string, batchId: string) => void;
}) {
	const queryClient = useQueryClient();
	const { acquisitionId, batchId, state } = acquisition;
	// The same DELETE cancels an active acquisition and dismisses a finished one.
	const cancel = useMutation({
		mutationFn: () => apiClient.cancelSoulseekAcquisition(acquisitionId),
		onSuccess: () => {
			if (state !== "failed") return;
			queryClient.setQueryData<SoulseekAcquisitionList>(
				soulseekQueryKeys.acquisitions,
				(list) =>
					list && {
						...list,
						items: list.items.filter(
							(item) => item.acquisitionId !== acquisitionId,
						),
					},
			);
		},
		onSettled: () =>
			queryClient.invalidateQueries({
				queryKey: soulseekQueryKeys.acquisitions,
			}),
	});
	const canCancel =
		state === "downloading" || state === "importing" || state === "review";
	// A failed acquisition's batch holds the files that did arrive; confirming
	// it imports them and the server then reads the acquisition as completed.
	const canReview = (state === "review" || state === "failed") && batchId;
	return (
		<li
			aria-label={acquisition.query}
			className="border-border/70 border-b px-3 py-3 last:border-b-0"
		>
			<div className="flex items-start gap-3">
				<div className="min-w-0 flex-1">
					<p
						className="truncate font-medium text-heading text-sm"
						title={acquisition.query}
					>
						{acquisition.query}
					</p>
					<p className="text-caption text-xs">
						{acquisition.scope === "album" ? "Album" : "Track"} ·{" "}
						{formatAcquisitionQuality(acquisition.quality)}
					</p>
					<AcquisitionStatus acquisition={acquisition} />
				</div>
				<div className="flex shrink-0 items-center gap-2">
					{canReview ? (
						<Button
							type="button"
							variant="outline"
							size="sm"
							disabled={isBusy}
							onClick={() => onReview(acquisitionId, batchId)}
						>
							Review import
						</Button>
					) : null}
					{canCancel ? (
						<Button
							type="button"
							variant="ghost"
							size="sm"
							aria-label={`Cancel ${acquisition.query}`}
							disabled={cancel.isPending}
							onClick={() => {
								if (window.confirm(CANCEL_ACQUISITION_PROMPT)) cancel.mutate();
							}}
						>
							Cancel
						</Button>
					) : null}
					{state === "failed" ? (
						<Button
							type="button"
							variant="ghost"
							size="sm"
							aria-label={`Dismiss ${acquisition.query}`}
							disabled={cancel.isPending}
							onClick={() => cancel.mutate()}
						>
							Dismiss
						</Button>
					) : null}
				</div>
			</div>
			{state === "downloading" ? (
				<ul aria-label="Files" className="mt-2 grid gap-2">
					{acquisition.files.map((file) => (
						<FileProgress key={file.filename} file={file} />
					))}
				</ul>
			) : null}
			{cancel.error ? (
				<p role="alert" className="mt-2 text-destructive text-sm">
					{cancelErrorMessage(cancel.error, state === "failed")}
				</p>
			) : null}
		</li>
	);
}

function AcquisitionStatus({
	acquisition,
}: {
	acquisition: SoulseekAcquisition;
}) {
	switch (acquisition.state) {
		case "downloading":
			return (
				<p aria-live="polite" className="text-caption text-xs tabular-nums">
					Downloading
					{acquisition.currentSource
						? ` from ${acquisition.currentSource.username}`
						: ""}{" "}
					· sources tried {acquisition.sourcesTried}/{acquisition.sourceCount}
				</p>
			);
		case "importing":
			return (
				<p aria-live="polite" className="text-caption text-xs">
					Importing…
				</p>
			);
		case "review":
			return <p className="text-foreground text-xs">Ready for review</p>;
		case "failed":
			return (
				<p className="text-destructive text-xs">
					Failed:{" "}
					{acquisition.failureReason ?? "no source of this version is left."}{" "}
					{acquisition.batchId
						? "Review the files that arrived, or choose another version with Install."
						: "Choose another version with Install."}
				</p>
			);
		default:
			return null;
	}
}

const fileStateLabels: Record<SoulseekAcquisitionFile["state"], string> = {
	queued: "Queued",
	downloading: "Downloading",
	downloaded: "Downloaded",
	staged: "Imported into preview",
	failed: "Failed",
};

function FileProgress({ file }: { file: SoulseekAcquisitionFile }) {
	const percent =
		file.state === "downloaded" || file.state === "staged"
			? 100
			: file.totalBytes > 0
				? Math.min(
						100,
						Math.round((file.bytesTransferred / file.totalBytes) * 100),
					)
				: 0;
	return (
		<li className="grid gap-1">
			<div className="flex items-center gap-3 text-xs">
				<span className="min-w-0 flex-1 truncate" title={file.filename}>
					{file.filename}
				</span>
				<span
					className={
						file.state === "failed"
							? "shrink-0 text-destructive"
							: "shrink-0 text-caption tabular-nums"
					}
				>
					{file.state === "downloading"
						? `${percent}%`
						: fileStateLabels[file.state]}
				</span>
			</div>
			<ProgressBar
				label={`${file.filename} download progress`}
				percent={percent}
				className="h-1"
			/>
		</li>
	);
}

function cancelErrorMessage(error: unknown, isDismiss: boolean): string {
	if (error instanceof ApiError && error.status === 409)
		return isDismiss
			? "This import is being committed and cannot be dismissed yet."
			: "This import is being committed and can no longer be cancelled.";
	return soulseekErrorMessage(error);
}
