import type { DesktopUpdateStatus } from "@repo/desktop-bridge";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { Button } from "#/components/ui/button";
import { Progress } from "#/components/ui/progress";
import {
	checkForDesktopUpdates,
	downloadDesktopUpdate,
	getDesktopUpdateStatus,
	installDesktopUpdate,
	listenForDesktopUpdateStatus,
} from "#/desktop/bridge";

const UPDATE_STATUS_KEY = ["desktop", "update-status"];

function logFailure(error: unknown) {
	console.error("Desktop update command failed", error);
}

/** The one action a state offers; `busy` shows it running, disabled. */
function updateAction(status: DesktopUpdateStatus) {
	const check = () => checkForDesktopUpdates().catch(logFailure);
	const download = () => downloadDesktopUpdate().catch(logFailure);
	const install = () => installDesktopUpdate().catch(logFailure);
	switch (status.status) {
		case "idle":
		case "up-to-date":
			return { label: "Check for updates", run: check, quiet: true };
		case "checking":
			return { label: "Checking for updates", quiet: true, busy: true };
		case "available":
			return { label: "Download update", run: download };
		case "downloaded":
			return { label: "Restart to update", run: install };
		case "installing":
			return { label: "Restarting to update", busy: true };
		case "failed":
			return status.action === "check"
				? { label: "Retry check", run: check }
				: { label: "Retry download", run: download };
		default:
			return null;
	}
}

function updateMessage(status: DesktopUpdateStatus): string | null {
	switch (status.status) {
		case "unavailable":
			return "Only installed releases of Harmony update.";
		case "up-to-date":
			return "Harmony is up to date.";
		case "available":
			return `Version ${status.version} is available.`;
		case "downloading":
			return `Downloading version ${status.version}.`;
		case "downloaded":
			return `Version ${status.version} is ready. Restarting stops playback.`;
		case "failed":
			return status.action === "check"
				? "Couldn't check for updates. Check your connection, then retry."
				: `Couldn't download version ${status.version}. Check your connection, then retry.`;
		default:
			return null;
	}
}

/** Which version this Desktop Client runs, and the way to the next one. */
export function DesktopUpdateSection() {
	const queryClient = useQueryClient();
	const { data: status } = useQuery({
		queryKey: UPDATE_STATUS_KEY,
		queryFn: getDesktopUpdateStatus,
	});

	useEffect(
		() =>
			listenForDesktopUpdateStatus((next) =>
				queryClient.setQueryData(UPDATE_STATUS_KEY, next),
			),
		[queryClient],
	);

	if (!status) return null;
	const action = updateAction(status);
	const message = updateMessage(status);
	return (
		<section
			aria-labelledby="desktop-update-heading"
			className="mb-8 flex flex-col gap-4"
		>
			<h2 id="desktop-update-heading" className="font-medium text-sm">
				Updates
			</h2>
			<dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
				<dt className="text-caption">Version</dt>
				<dd className="min-w-0 truncate">{status.currentVersion}</dd>
			</dl>
			{/* Always mounted, so a screen reader announces each change. */}
			<p
				role={status.status === "failed" ? "alert" : "status"}
				className={
					status.status === "failed"
						? "text-destructive text-sm"
						: "text-caption text-sm empty:hidden"
				}
			>
				{message}
			</p>
			{status.status === "downloading" ? (
				<div className="flex max-w-md items-center gap-3">
					<Progress
						value={status.percent}
						aria-label="Update download"
						className="w-full"
					/>
					<span className="text-sm tabular-nums">{status.percent}%</span>
				</div>
			) : null}
			{action ? (
				<Button
					type="button"
					variant={action.quiet ? "outline" : "default"}
					className="self-start"
					disabled={action.busy}
					aria-busy={action.busy}
					onClick={action.run}
				>
					{action.label}
				</Button>
			) : null}
		</section>
	);
}
