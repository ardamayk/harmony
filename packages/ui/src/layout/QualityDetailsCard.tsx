import { type ReactNode, useId, useState } from "react";
import { cn } from "../lib/utils";
import type { OutputMode, ProcessingState } from "../playback/processing";
import {
	derivePlaybackTelemetryStatus,
	describePlaybackTelemetry,
	formatTelemetryStatus,
	type PlaybackTelemetry,
} from "../playback/telemetry";

const OUTPUT_MODE_LABELS: Record<OutputMode, string> = {
	system: "Normal",
	"direct-alsa": "Exclusive",
	"adaptive-system-rate": "Adaptive",
};

const REPLAY_GAIN_LABELS: Record<
	ProcessingState["effectiveReplayGainMode"],
	string
> = {
	off: "Off",
	track: "Track",
	album: "Album",
	"track-fallback": "Track (album gain missing)",
	unavailable: "Unavailable",
	unknown: "Unknown",
};

export type QualityDetailRow = { label: string; value: string };

/** Rows for the hover card; exported so tests can pin the mapping. */
export function buildQualityDetailRows({
	telemetry,
	processing,
	outputMode,
}: {
	telemetry: PlaybackTelemetry | null;
	processing: ProcessingState | null;
	outputMode: OutputMode | null;
}): QualityDetailRow[] {
	const rows: QualityDetailRow[] = [];
	if (telemetry) {
		const descriptions = describePlaybackTelemetry(telemetry);
		rows.push({ label: "Source", value: descriptions.source });
		rows.push({ label: "Decoder", value: descriptions.decoder });
		rows.push({ label: "Output", value: descriptions.device });
		// The browser fallback only guesses the status from the volume level.
		if (telemetry.system.kind !== "browser-managed") {
			rows.push({
				label: "Status",
				value: formatTelemetryStatus(derivePlaybackTelemetryStatus(telemetry)),
			});
		}
		const resampling =
			telemetry.device.isResampling ?? telemetry.system.isResampling;
		if (resampling !== null) {
			rows.push({ label: "Resampling", value: resampling ? "Yes" : "No" });
		}
	}
	if (outputMode) {
		rows.push({ label: "Output mode", value: OUTPUT_MODE_LABELS[outputMode] });
	}
	if (processing) {
		rows.push({
			label: "ReplayGain",
			value: REPLAY_GAIN_LABELS[processing.effectiveReplayGainMode],
		});
		rows.push({
			label: "Equalizer",
			value: processing.equalizer.isEnabled ? "On" : "Off",
		});
	}
	// Browser playback cannot observe the decoder or device, so those rows
	// would only ever say "Unknown"; leave them out instead of padding the card.
	return rows.filter((row) => row.value !== "Unknown");
}

/**
 * Wraps the quality pill and reveals the stream details on hover or focus.
 * The card hides itself as soon as the pill is clicked, and stays hidden
 * while the output-mode menu is open, so the two never stack.
 */
export function QualityDetailsCard({
	rows,
	children,
}: {
	rows: QualityDetailRow[];
	children: ReactNode;
}) {
	const id = useId();
	const [suppressed, setSuppressed] = useState(false);
	if (rows.length === 0) return <>{children}</>;
	return (
		// biome-ignore lint/a11y/noStaticElementInteractions: hover wrapper only; the pill inside is the control.
		<div
			className="group/quality relative flex shrink-0 items-center"
			onPointerDown={() => setSuppressed(true)}
			onPointerLeave={() => setSuppressed(false)}
			onBlur={(event) => {
				if (!event.currentTarget.contains(event.relatedTarget as Node | null))
					setSuppressed(false);
			}}
		>
			{children}
			<div
				id={id}
				role="tooltip"
				data-testid="quality-details"
				className={cn(
					// An open menu inside (the output-mode menu) always wins over the
					// card, however the menu was opened or wherever the pointer is.
					"pointer-events-none absolute right-0 bottom-full z-40 mb-2 w-64 rounded-md border border-border bg-popover p-3 text-popover-foreground text-xs opacity-0 shadow-lg group-has-[[aria-expanded=true]]/quality:invisible",
					!suppressed &&
						"group-focus-within/quality:opacity-100 group-hover/quality:opacity-100",
				)}
			>
				<dl className="grid grid-cols-media gap-x-3 gap-y-1">
					{rows.map((row) => (
						<div key={row.label} className="contents">
							<dt className="text-caption">{row.label}</dt>
							<dd className="min-w-0 break-words font-medium">{row.value}</dd>
						</div>
					))}
				</dl>
			</div>
		</div>
	);
}
