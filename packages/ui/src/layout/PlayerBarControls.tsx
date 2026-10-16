import {
	AudioLines,
	Disc3,
	Infinity as InfinityIcon,
	Keyboard,
	ListMusic,
	MicVocal,
	Pause,
	PictureInPicture2,
	Play,
	Repeat,
	Repeat1,
	Shuffle,
	SkipBack,
	SkipForward,
	Volume1,
	Volume2,
	VolumeX,
} from "lucide-react";
import {
	type CSSProperties,
	type ReactNode,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { cn } from "../lib/utils";
import type { RepeatMode } from "../playback/PlaybackProvider";
import {
	type QualityDetailRow,
	QualityDetailsCard,
} from "./QualityDetailsCard";
import { SeekBar } from "./SeekBar";

const CONTROL_BUTTON_CLASS =
	"inline-flex size-6 items-center justify-center rounded-sm text-player-foreground hover:text-player-control-primary disabled:opacity-40";
const CONTROL_ICON_CLASS = "size-[18px]";
// The right cluster sits alone at the edge, so its icons run one step larger.
const SIDE_ICON_CLASS = "size-5";
const SIDE_BUTTON_CLASS = "size-7";
const ACTIVE_CONTROL_BUTTON_CLASS = "text-player-control-primary";
const VOLUME_WHEEL_STEP = 0.05;
const MAX_QUEUE_BADGE_COUNT = 9;

type PlaybackControlsProps = {
	isRadioPlaying: boolean;
	isPlaying: boolean;
	hasPlayableSource: boolean;
	hasCurrentTrack: boolean;
	currentTime: number;
	effectiveDuration: number;
	bufferedEnd?: number | null;
	waveform?: number[] | null;
	showHoverTimestamp?: boolean;
	keyboardStepSeconds?: number;
	keyboardLargeStepSeconds?: number;
	shuffleEnabled: boolean;
	repeatMode: RepeatMode;
	onTogglePlay: () => void;
	onToggleShuffle: () => void;
	onCycleRepeatMode: () => void;
	onPrevious: () => void;
	onNext: () => void;
	onSeek: (seconds: number) => void;
};

export function PlaybackControls(props: PlaybackControlsProps) {
	return (
		<section
			aria-label="Playback controls"
			className="flex min-w-px max-w-[448px] translate-y-[5px] flex-[1_0_0] flex-col items-center justify-center justify-self-center"
		>
			<TransportControls {...props} />
			<PlaybackProgress {...props} />
		</section>
	);
}

function TransportControls({
	isPlaying,
	hasPlayableSource,
	hasCurrentTrack,
	shuffleEnabled,
	repeatMode,
	onTogglePlay,
	onToggleShuffle,
	onCycleRepeatMode,
	onPrevious,
	onNext,
}: PlaybackControlsProps) {
	return (
		<div className="flex items-center gap-6">
			<ShuffleButton
				isEnabled={shuffleEnabled}
				disabled={!hasCurrentTrack}
				onClick={onToggleShuffle}
			/>
			<QueueNavigationButton
				label="Previous"
				disabled={!hasCurrentTrack}
				onPlay={onPrevious}
			/>
			<PrimaryPlaybackButton
				isPlaying={isPlaying}
				disabled={!hasPlayableSource}
				onClick={onTogglePlay}
			/>
			<QueueNavigationButton
				label="Next"
				disabled={!hasCurrentTrack}
				onPlay={onNext}
			/>
			<RepeatButton
				repeatMode={repeatMode}
				disabled={!hasCurrentTrack}
				onClick={onCycleRepeatMode}
			/>
		</div>
	);
}

function QueueNavigationButton({
	label,
	disabled,
	onPlay,
}: {
	label: "Previous" | "Next";
	disabled: boolean;
	onPlay: () => void;
}) {
	const Icon = label === "Previous" ? SkipBack : SkipForward;
	return (
		<button
			type="button"
			data-player-control
			className={CONTROL_BUTTON_CLASS}
			onClick={() => {
				if (!disabled) onPlay();
			}}
			disabled={disabled}
			aria-label={label}
		>
			<Icon className={CONTROL_ICON_CLASS} />
		</button>
	);
}

function ShuffleButton({
	isEnabled,
	disabled,
	onClick,
}: {
	isEnabled: boolean;
	disabled: boolean;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			data-player-control
			className={cn(
				CONTROL_BUTTON_CLASS,
				isEnabled && ACTIVE_CONTROL_BUTTON_CLASS,
			)}
			aria-label={isEnabled ? "Shuffle on" : "Shuffle off"}
			onClick={onClick}
			disabled={disabled}
		>
			<Shuffle className={CONTROL_ICON_CLASS} />
		</button>
	);
}

function PrimaryPlaybackButton({
	isPlaying,
	disabled,
	onClick,
}: {
	isPlaying: boolean;
	disabled: boolean;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			data-player-control
			className="inline-flex size-11 items-center justify-center rounded-full bg-player-control-primary text-player-control-primary-foreground shadow-[0px_10px_15px_-3px_var(--player-control-shadow),0px_4px_6px_-4px_var(--player-control-shadow)] transition hover:scale-105 hover:opacity-95 disabled:opacity-50 disabled:hover:scale-100"
			onClick={onClick}
			disabled={disabled}
			aria-label={isPlaying ? "Pause" : "Play"}
		>
			{isPlaying ? (
				<Pause
					key="pause"
					className={cn(CONTROL_ICON_CLASS, "player-glyph-enter")}
				/>
			) : (
				<Play
					key="play"
					className={cn(CONTROL_ICON_CLASS, "player-glyph-enter ml-0.5")}
				/>
			)}
		</button>
	);
}

function RepeatButton({
	repeatMode,
	disabled,
	onClick,
}: {
	repeatMode: RepeatMode;
	disabled: boolean;
	onClick: () => void;
}) {
	const label =
		repeatMode === "off"
			? "Repeat off"
			: repeatMode === "once"
				? "Repeat once"
				: "Repeat loop";
	return (
		<button
			type="button"
			data-player-control
			className={cn(
				CONTROL_BUTTON_CLASS,
				repeatMode !== "off" && ACTIVE_CONTROL_BUTTON_CLASS,
				"relative",
			)}
			aria-label={label}
			onClick={onClick}
			disabled={disabled}
		>
			{/* Repeat once swaps the glyph, so the state reads from its shape. */}
			{repeatMode === "once" ? (
				<Repeat1 className={CONTROL_ICON_CLASS} />
			) : (
				<Repeat className={CONTROL_ICON_CLASS} />
			)}
			{repeatMode === "loop" ? (
				<span
					className="-right-1 -bottom-1 absolute flex size-4 items-center justify-center rounded-full bg-primary text-primary-foreground"
					role="img"
					aria-label="Repeat infinitely"
				>
					<InfinityIcon className="size-2.5" />
				</span>
			) : null}
		</button>
	);
}

function PlaybackProgress({
	isRadioPlaying,
	hasCurrentTrack,
	currentTime,
	effectiveDuration,
	bufferedEnd = null,
	waveform = null,
	showHoverTimestamp = true,
	keyboardStepSeconds,
	keyboardLargeStepSeconds,
	onSeek,
}: {
	isRadioPlaying: boolean;
	hasCurrentTrack: boolean;
	currentTime: number;
	effectiveDuration: number;
	bufferedEnd?: number | null;
	waveform?: number[] | null;
	showHoverTimestamp?: boolean;
	keyboardStepSeconds?: number;
	keyboardLargeStepSeconds?: number;
	onSeek: (seconds: number) => void;
}) {
	return (
		<div className="mt-1 flex w-full min-w-0 items-center gap-2 text-xs tabular-nums">
			<span className="w-8 shrink-0 text-right text-player-foreground">
				{isRadioPlaying ? "LIVE" : formatTime(currentTime)}
			</span>
			{isRadioPlaying ? (
				<div className="h-1 min-w-0 flex-1 rounded-full bg-player-live-progress/45" />
			) : (
				<SeekBar
					currentTime={currentTime}
					duration={effectiveDuration}
					bufferedEnd={bufferedEnd}
					waveform={waveform}
					disabled={!hasCurrentTrack}
					showHoverTimestamp={showHoverTimestamp}
					keyboardStepSeconds={keyboardStepSeconds}
					keyboardLargeStepSeconds={keyboardLargeStepSeconds}
					onSeek={onSeek}
				/>
			)}
			{isRadioPlaying ? null : (
				<span className="w-8 shrink-0 text-player-foreground">
					{formatTime(effectiveDuration)}
				</span>
			)}
		</div>
	);
}

type VolumeAndQueueControlsProps = {
	qualityLabel: string;
	isLossless: boolean;
	/** Stream details shown on hover; empty hides the card. */
	qualityDetailRows?: QualityDetailRow[];
	/** Shows a small badge when the engine plays tracks without gaps. */
	isGapless?: boolean;
	volume: number;
	signalControl?: ReactNode;
	isQueueOpen?: boolean;
	upcomingCount?: number;
	hasQueueItems?: boolean;
	onToggleQueue: () => void;
	/** Absent while nothing is playing; the Lyrics button is then disabled. */
	onOpenLyrics?: () => void;
	onOpenHelp?: () => void;
	/** Desktop only; absent hides the button. */
	onToggleMiniPlayer?: () => void;
	onToggleMute: () => void;
	onVolumeChange: (value: number) => void;
};

export function QualityIconFor({ isLossless }: { isLossless: boolean }) {
	const Icon = isLossless ? Disc3 : AudioLines;
	return <Icon className="size-4 shrink-0" />;
}

export function VolumeAndQueueControls({
	qualityLabel,
	isLossless,
	qualityDetailRows = [],
	isGapless = false,
	volume,
	signalControl,
	isQueueOpen = false,
	upcomingCount = 0,
	hasQueueItems = upcomingCount > 0,
	onToggleQueue,
	onOpenLyrics,
	onOpenHelp,
	onToggleMiniPlayer,
	onToggleMute,
	onVolumeChange,
}: VolumeAndQueueControlsProps) {
	const QualityIcon = isLossless ? Disc3 : AudioLines;
	return (
		<section
			aria-label="Volume and queue"
			className="flex min-w-[200px] flex-[1_0_0] items-center justify-end gap-3 justify-self-end @min-[66rem]/player-bar:gap-4"
		>
			{isGapless ? (
				<span
					data-testid="gapless-badge"
					className="hidden size-7 shrink-0 items-center justify-center text-player-foreground @min-[74rem]/player-bar:inline-flex"
					role="img"
					aria-label="Gapless playback enabled"
					title="Tracks play back to back without a gap"
				>
					<InfinityIcon className={SIDE_ICON_CLASS} aria-hidden />
				</span>
			) : null}
			<QualityDetailsCard rows={qualityDetailRows}>
				{signalControl ?? (
					<span
						role="note"
						className="inline-flex h-8 shrink-0 items-center gap-2 rounded-lg border border-sidebar-border bg-player-pill px-4 text-player-foreground text-xs"
						title={qualityLabel}
						aria-label={`Quality ${qualityLabel}`}
					>
						<QualityIcon className="size-4 shrink-0" />
						<span className="hidden font-medium tabular-nums @min-[66rem]/player-bar:inline">
							{qualityLabel}
						</span>
					</span>
				)}
			</QualityDetailsCard>
			<VolumeControl
				volume={volume}
				onToggleMute={onToggleMute}
				onVolumeChange={onVolumeChange}
			/>
			<button
				type="button"
				data-player-control
				className={cn(
					CONTROL_BUTTON_CLASS,
					SIDE_BUTTON_CLASS,
					"hidden shrink-0 sm:inline-flex",
				)}
				onClick={onOpenLyrics}
				disabled={!onOpenLyrics}
				aria-label="Lyrics"
			>
				<MicVocal className={SIDE_ICON_CLASS} />
			</button>
			<button
				type="button"
				data-player-control
				className={cn(
					CONTROL_BUTTON_CLASS,
					SIDE_BUTTON_CLASS,
					"relative hidden shrink-0 sm:inline-flex",
					isQueueOpen && ACTIVE_CONTROL_BUTTON_CLASS,
				)}
				onClick={(event) => {
					// Release pointer focus so Space returns to playback shortcuts.
					if (event.detail > 0) event.currentTarget.blur();
					onToggleQueue();
				}}
				aria-label="Toggle queue panel"
				aria-expanded={isQueueOpen}
			>
				<ListMusic className={SIDE_ICON_CLASS} />
				{!isQueueOpen && hasQueueItems ? (
					<span className="-top-1 -right-1.5 absolute flex h-4 min-w-4 items-center justify-center rounded-full bg-player-control-primary px-1 font-semibold text-xs text-player-control-primary-foreground">
						{upcomingCount > MAX_QUEUE_BADGE_COUNT
							? `${MAX_QUEUE_BADGE_COUNT}+`
							: upcomingCount}
					</span>
				) : null}
			</button>
			{onToggleMiniPlayer ? (
				<button
					type="button"
					data-player-control
					className={cn(
						CONTROL_BUTTON_CLASS,
						SIDE_BUTTON_CLASS,
						"hidden shrink-0 sm:inline-flex",
					)}
					onClick={onToggleMiniPlayer}
					aria-label="Mini player"
				>
					<PictureInPicture2 className={SIDE_ICON_CLASS} />
				</button>
			) : null}
			{onOpenHelp ? (
				<button
					type="button"
					data-player-control
					className={cn(
						CONTROL_BUTTON_CLASS,
						SIDE_BUTTON_CLASS,
						"hidden shrink-0 lg:inline-flex",
					)}
					onClick={onOpenHelp}
					aria-label="Keyboard shortcuts"
				>
					<Keyboard className={SIDE_ICON_CLASS} />
				</button>
			) : null}
		</section>
	);
}

function VolumeControl({
	volume,
	onToggleMute,
	onVolumeChange,
}: {
	volume: number;
	onToggleMute: () => void;
	onVolumeChange: (value: number) => void;
}) {
	const VolumeIcon = volume <= 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;
	// Touch devices have no hover, so a tap opens the popover instead of muting.
	const [touchOpen, setTouchOpen] = useState(false);
	const lastPointerWasTouchRef = useRef(false);

	const rootRef = useRef<HTMLDivElement>(null);
	// React registers wheel listeners as passive, so a native listener is the
	// only way to stop the page behind the bar from scrolling too.
	const volumeRef = useRef(volume);
	useLayoutEffect(() => {
		volumeRef.current = volume;
	});
	useEffect(() => {
		const root = rootRef.current;
		if (!root) return undefined;
		const adjustFromWheel = (event: WheelEvent) => {
			if (event.deltaY === 0) return;
			event.preventDefault();
			const next =
				volumeRef.current + (event.deltaY < 0 ? 1 : -1) * VOLUME_WHEEL_STEP;
			onVolumeChange(Math.min(1, Math.max(0, Number(next.toFixed(2)))));
		};
		root.addEventListener("wheel", adjustFromWheel, { passive: false });
		return () => root.removeEventListener("wheel", adjustFromWheel);
	}, [onVolumeChange]);
	useEffect(() => {
		if (!touchOpen) return undefined;
		const closeOnOutsideTap = (event: PointerEvent) => {
			if (!rootRef.current?.contains(event.target as Node)) setTouchOpen(false);
		};
		document.addEventListener("pointerdown", closeOnOutsideTap);
		return () => document.removeEventListener("pointerdown", closeOnOutsideTap);
	}, [touchOpen]);

	return (
		<div ref={rootRef} className="group relative flex shrink-0 items-center">
			<button
				type="button"
				data-player-control
				className={cn(
					CONTROL_BUTTON_CLASS,
					SIDE_BUTTON_CLASS,
					"group-focus-within:text-player-control-primary group-hover:text-player-control-primary",
				)}
				aria-label={volume <= 0 ? "Unmute" : "Mute"}
				onPointerDown={(event) => {
					lastPointerWasTouchRef.current = event.pointerType === "touch";
				}}
				onClick={() => {
					if (lastPointerWasTouchRef.current) {
						setTouchOpen((open) => !open);
						return;
					}
					onToggleMute();
				}}
			>
				<VolumeIcon className={SIDE_ICON_CLASS} />
			</button>
			<div
				data-testid="volume-popover"
				className={cn(
					"pointer-events-none absolute bottom-full left-1/2 z-40 -translate-x-1/2 pb-3 opacity-0 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100",
					touchOpen && "pointer-events-auto opacity-100",
				)}
			>
				<div className="flex h-32 w-9 items-center justify-center rounded-lg border border-player-border bg-player px-2 py-3 shadow-[0_8px_24px_-4px_var(--player-shadow)]">
					<div className="relative h-full w-4">
						<input
							type="range"
							min={0}
							max={1}
							step={0.01}
							value={volume}
							onChange={(event) => onVolumeChange(Number(event.target.value))}
							className="player-volume-slider"
							style={
								{
									"--volume-level": `${Math.round(volume * 100)}%`,
								} as CSSProperties
							}
							aria-label="Volume"
							aria-orientation="vertical"
						/>
					</div>
				</div>
			</div>
		</div>
	);
}

function formatTime(seconds: number): string {
	if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
	const minutes = Math.floor(seconds / 60);
	const remainingSeconds = Math.floor(seconds % 60);
	return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}
