import type { PlaybackError } from "../playback/PlaybackEngine";
import type { PlaybackErrorRecovery } from "../playback/PlaybackProvider";

/** Second line of the banner, explaining what the probe found. */
function describePlaybackErrorCause(
	cause: PlaybackErrorRecovery["cause"],
): string | null {
	switch (cause) {
		case "file-missing":
			return "The file is missing on the server.";
		case "network":
			return "The server could not be reached.";
		default:
			return null;
	}
}

/**
 * Inline replacement for the bare error line above the bar: names the cause
 * when known, offers Retry and Skip, and counts down an automatic skip.
 */
export function PlaybackErrorBanner({
	error,
	recovery,
}: {
	error: PlaybackError;
	recovery: PlaybackErrorRecovery | null;
}) {
	const causeText = describePlaybackErrorCause(recovery?.cause ?? null);
	return (
		<div
			role="alert"
			data-testid="playback-error-banner"
			className="absolute bottom-full left-1/2 mb-2 flex max-w-lg -translate-x-1/2 flex-wrap items-center gap-x-3 gap-y-1 rounded-md border border-destructive/40 bg-popover px-3 py-2 text-sm shadow-lg"
		>
			<div className="min-w-0 flex-1">
				<p className="text-destructive">{error.message}</p>
				{causeText ? <p className="text-caption text-xs">{causeText}</p> : null}
			</div>
			{recovery ? (
				<div className="flex shrink-0 items-center gap-2">
					{recovery.countdownSeconds !== null && recovery.canSkip ? (
						<span className="text-caption text-xs tabular-nums" aria-live="off">
							Skipping in {recovery.countdownSeconds}s
						</span>
					) : null}
					<button
						type="button"
						data-player-control
						className="rounded-sm border border-border px-2 py-1 text-xs hover:bg-muted"
						onClick={recovery.retry}
					>
						Retry
					</button>
					{recovery.canSkip ? (
						<button
							type="button"
							data-player-control
							className="rounded-sm bg-primary px-2 py-1 text-primary-foreground text-xs hover:opacity-90"
							onClick={recovery.skip}
						>
							Skip
						</button>
					) : null}
					{recovery.countdownSeconds !== null && recovery.canSkip ? (
						<button
							type="button"
							data-player-control
							className="rounded-sm px-2 py-1 text-caption text-xs hover:bg-muted"
							onClick={recovery.cancelCountdown}
						>
							Wait
						</button>
					) : null}
				</div>
			) : null}
		</div>
	);
}
