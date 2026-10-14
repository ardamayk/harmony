import type React from "react";
import {
	type CSSProperties,
	type PointerEvent as ReactPointerEvent,
	useEffect,
	useEffectEvent,
	useId,
	useRef,
	useState,
} from "react";
import { cn } from "../lib/utils";

export function formatSeekTime(seconds: number): string {
	if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
	const minutes = Math.floor(seconds / 60);
	const remainingSeconds = Math.floor(seconds % 60);
	return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

/** Fraction of the bar under a pointer, clamped so drags past the ends stick. */
export function fractionForPointer(
	clientX: number,
	rect: { left: number; width: number },
): number {
	if (rect.width <= 0) return 0;
	return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
}

type SeekBarProps = {
	currentTime: number;
	duration: number;
	/** End of the buffered range, in seconds; null hides the buffered layer. */
	bufferedEnd?: number | null;
	/** Normalized peaks (0..255) drawn behind the track; null keeps the thin bar. */
	waveform?: number[] | null;
	disabled?: boolean;
	showHoverTimestamp?: boolean;
	/** Arrow-key step on the focused bar; Shift uses the large step. */
	keyboardStepSeconds?: number;
	keyboardLargeStepSeconds?: number;
	onSeek: (seconds: number) => void;
	className?: string;
};

/**
 * Seek bar with a tall hit area, a buffered layer, a hover timestamp and
 * pointer-capture scrubbing. The native range input stays for keyboard and
 * screen-reader use; pointer input is handled on the wrapper so a drag that
 * leaves the bar keeps scrubbing and clamps at the ends.
 */
export function SeekBar({
	currentTime,
	duration,
	bufferedEnd = null,
	waveform = null,
	disabled = false,
	showHoverTimestamp = true,
	keyboardStepSeconds = 5,
	keyboardLargeStepSeconds = 30,
	onSeek,
	className,
}: SeekBarProps) {
	const hasWaveform = Boolean(waveform && waveform.length > 0);
	// The global shortcuts leave focused range inputs alone, so the bar
	// handles its own arrows and applies the same seek steps as the shortcuts.
	const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
		if (!canSeek) return;
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const step = event.shiftKey
			? keyboardLargeStepSeconds
			: keyboardStepSeconds;
		const delta = event.key === "ArrowRight" ? step : -step;
		onSeek(Math.min(duration, Math.max(0, currentTime + delta)));
	};
	const rootRef = useRef<HTMLDivElement>(null);
	const inputRef = useRef<HTMLInputElement>(null);
	const tooltipId = useId();
	const [hoverFraction, setHoverFraction] = useState<number | null>(null);
	const [scrubFraction, setScrubFraction] = useState<number | null>(null);
	const scrubPointerIdRef = useRef<number | null>(null);

	const canSeek = !disabled && duration > 0;
	const playedFraction = duration > 0 ? currentTime / duration : 0;
	const displayedFraction = scrubFraction ?? playedFraction;
	const bufferedFraction =
		bufferedEnd !== null && duration > 0
			? Math.min(1, Math.max(0, bufferedEnd / duration))
			: 0;

	const fractionAt = (clientX: number) => {
		const rect = rootRef.current?.getBoundingClientRect();
		return rect ? fractionForPointer(clientX, rect) : 0;
	};

	const cancelScrub = () => {
		const pointerId = scrubPointerIdRef.current;
		if (pointerId !== null) {
			try {
				rootRef.current?.releasePointerCapture(pointerId);
			} catch {
				// The pointer may already be gone; nothing to release.
			}
		}
		scrubPointerIdRef.current = null;
		setScrubFraction(null);
	};

	const onScrubKeyDown = useEffectEvent((event: KeyboardEvent) => {
		if (event.key === "Escape") cancelScrub();
	});
	const isScrubbing = scrubFraction !== null;
	useEffect(() => {
		if (!isScrubbing) return undefined;
		document.addEventListener("keydown", onScrubKeyDown);
		return () => document.removeEventListener("keydown", onScrubKeyDown);
	}, [isScrubbing]);

	const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
		if (!canSeek || event.button !== 0) return;
		event.preventDefault();
		inputRef.current?.focus({ preventScroll: true });
		rootRef.current?.setPointerCapture(event.pointerId);
		scrubPointerIdRef.current = event.pointerId;
		setScrubFraction(fractionAt(event.clientX));
	};

	const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
		const fraction = fractionAt(event.clientX);
		if (scrubPointerIdRef.current === event.pointerId) {
			setScrubFraction(fraction);
		}
		if (event.pointerType !== "touch") setHoverFraction(fraction);
	};

	const handlePointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
		if (scrubPointerIdRef.current !== event.pointerId) return;
		const fraction = fractionAt(event.clientX);
		cancelScrub();
		if (canSeek) onSeek(fraction * duration);
	};

	const handlePointerCancel = (event: ReactPointerEvent<HTMLDivElement>) => {
		if (scrubPointerIdRef.current === event.pointerId) cancelScrub();
	};

	const tooltipFraction = scrubFraction ?? hoverFraction;
	const showTooltip = showHoverTimestamp && canSeek && tooltipFraction !== null;

	return (
		<div
			ref={rootRef}
			data-testid="seek-bar"
			data-scrubbing={scrubFraction !== null ? "true" : undefined}
			data-waveform={hasWaveform ? "true" : undefined}
			className={cn(
				"relative flex min-w-0 flex-1 touch-none items-center",
				hasWaveform ? "h-8" : "h-6",
				className,
			)}
			onPointerDown={handlePointerDown}
			onPointerMove={handlePointerMove}
			onPointerUp={handlePointerUp}
			onPointerCancel={handlePointerCancel}
			onPointerLeave={() => setHoverFraction(null)}
		>
			{hasWaveform && waveform ? (
				<WaveformBars
					peaks={waveform}
					playedFraction={displayedFraction}
					bufferedFraction={bufferedFraction}
				/>
			) : null}
			<div
				aria-hidden
				data-testid="seek-buffered"
				className="pointer-events-none absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 overflow-hidden rounded-full"
			>
				<div
					className="h-full rounded-full bg-[var(--player-foreground)]/25"
					style={{ width: `${(bufferedFraction * 100).toFixed(2)}%` }}
				/>
			</div>
			<input
				ref={inputRef}
				type="range"
				min={0}
				max={1}
				step={0.001}
				value={displayedFraction}
				onChange={(event) => {
					// Home/End and page keys still arrive here; arrows are handled
					// above with the configured steps.
					if (canSeek) onSeek(Number(event.target.value) * duration);
				}}
				onKeyDown={handleKeyDown}
				className={cn(
					"player-seek-slider relative min-w-0 flex-1 disabled:opacity-100",
					hasWaveform && "player-seek-slider--waveform",
				)}
				style={
					{
						"--seek-level": `${(displayedFraction * 100).toFixed(2)}%`,
					} as CSSProperties
				}
				disabled={disabled}
				aria-label="Seek"
				aria-valuetext={`${formatSeekTime(displayedFraction * duration)} of ${formatSeekTime(duration)}`}
				aria-describedby={showTooltip ? tooltipId : undefined}
			/>
			{showTooltip ? (
				<span
					id={tooltipId}
					role="tooltip"
					data-testid="seek-tooltip"
					className="pointer-events-none absolute bottom-full mb-1 -translate-x-1/2 rounded bg-popover px-1.5 py-0.5 text-[11px] text-popover-foreground tabular-nums shadow"
					style={{ left: `${(tooltipFraction * 100).toFixed(2)}%` }}
				>
					{formatSeekTime(tooltipFraction * duration)}
				</span>
			) : null}
		</div>
	);
}

/**
 * Waveform bars behind the seek track. Played bars take the accent colour
 * through a clip on a second, coloured copy; buffered bars are lighter.
 */
function WaveformBars({
	peaks,
	playedFraction,
	bufferedFraction,
}: {
	peaks: number[];
	playedFraction: number;
	bufferedFraction: number;
}) {
	const bars = peaks.map((peak, index) => {
		const height = Math.max(1, Math.round((peak / 255) * 100));
		return (
			<rect
				// biome-ignore lint/suspicious/noArrayIndexKey: peaks are positional
				key={index}
				x={index}
				y={(100 - height) / 2}
				width={0.7}
				height={height}
				rx={0.35}
			/>
		);
	});
	const viewBox = `0 0 ${peaks.length} 100`;
	return (
		<svg
			aria-hidden="true"
			role="presentation"
			data-testid="seek-waveform"
			className="pointer-events-none absolute inset-0 h-full w-full"
			viewBox={viewBox}
			preserveAspectRatio="none"
		>
			<title>Waveform</title>
			<g className="fill-[var(--player-foreground)] opacity-25">{bars}</g>
			<g
				className="fill-[var(--player-foreground)] opacity-20"
				style={{
					clipPath: `inset(0 ${(100 - bufferedFraction * 100).toFixed(2)}% 0 0)`,
				}}
			>
				{bars}
			</g>
			<g
				data-testid="seek-waveform-played"
				className="fill-[var(--player-live-progress)]"
				style={{
					clipPath: `inset(0 ${(100 - playedFraction * 100).toFixed(2)}% 0 0)`,
				}}
			>
				{bars}
			</g>
		</svg>
	);
}
