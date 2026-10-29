import { type CSSProperties, useEffect, useState } from "react";
import { cn } from "#/lib/utils";
import type { ImportFileEntry } from "./-managed-import-workflow";

export function ProgressBar({
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

const BYTES_PER_MIB = 1024 * 1024;
const PROGRESS_CLOCK_INTERVAL_MS = 1000;
const PHASE_LABELS = {
	queued: "Queued",
	uploading: "Uploading…",
	validating: "Validating…",
	ready: "Ready",
	failed: "Failed",
	committing: "Importing…",
	completed: "Completed",
	retry_wait: "Waiting to retry…",
};

export function importPhaseLabel(entry: ImportFileEntry): string {
	return PHASE_LABELS[entry.phase ?? "queued"];
}

function formatBytes(bytes: number) {
	return `${(bytes / BYTES_PER_MIB).toFixed(1)} MiB`;
}

export function ImportTransferDetails({
	entry,
	isExpanded = false,
}: {
	entry: ImportFileEntry;
	isExpanded?: boolean;
}) {
	const [now, setNow] = useState(Date.now);
	const isActive =
		entry.phase === "retry_wait" ||
		(isExpanded && entry.state === "unresolved" && entry.phase !== "queued");
	useEffect(() => {
		if (!isActive) return;
		const tick = () => setNow(Date.now());
		const catchUp = setTimeout(tick, 0);
		const timer = setInterval(tick, PROGRESS_CLOCK_INTERVAL_MS);
		return () => {
			clearTimeout(catchUp);
			clearInterval(timer);
		};
	}, [isActive]);
	const elapsed = Math.max(
		0,
		Math.floor((now - (entry.startedAt ?? now)) / 1000),
	);
	const bytes = entry.transferredBytes ?? 0;
	return (
		<div className="text-caption text-xs">
			{!isExpanded && entry.phase === "retry_wait" ? (
				<p>
					Retrying in{" "}
					{Math.max(0, Math.ceil(((entry.retryAt ?? now) - now) / 1000))}s ·{" "}
					{entry.retryCount}/3
				</p>
			) : null}
			{!isExpanded && entry.phase === "uploading" ? (
				<p>
					{formatBytes(bytes)} / {formatBytes(entry.file.size)}
				</p>
			) : null}
			{isExpanded && entry.startedAt ? (
				<div>
					<p>
						{entry.state === "unresolved"
							? `${elapsed}s elapsed`
							: `${formatBytes(entry.file.size)} file`}{" "}
						· {entry.retryCount ?? 0} retries
					</p>
					{entry.phase === "uploading" && elapsed > 0 ? (
						<p>{formatBytes(bytes / elapsed)}/s average</p>
					) : null}
				</div>
			) : null}
		</div>
	);
}
