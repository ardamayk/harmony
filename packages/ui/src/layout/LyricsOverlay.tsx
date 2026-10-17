import type { Track } from "@repo/api-client";
import { ChevronDown } from "lucide-react";
import {
	type CSSProperties,
	type ReactNode,
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import {
	activeLyricIndex,
	type ParsedLyrics,
	parseLyrics,
} from "../lib/lyrics";
import { useFocusTrap } from "../lib/use-focus-trap";
import { cn } from "../lib/utils";
import { usePlayback } from "../playback/PlaybackProvider";
import { CurrentTrackPanel } from "./CurrentTrackPanel";
import { QueuePreview } from "./QueuePreview";
import { WindowControlButtons, type WindowControls } from "./TopNav";

/** Matches --animate-lyrics-view-exit in web/src/styles.css. */
const EXIT_MS = 250;

type LyricsState =
	| { status: "loading" }
	| { status: "loaded"; lyrics: string }
	| { status: "failed" };

/**
 * Full-screen listening view with the current Track, its lyrics and a
 * preview of the Queue around it.
 */
export function LyricsOverlay({
	track,
	coverUrl,
	accent = {},
	windowControls,
	loadLyrics,
	onClose,
}: {
	track: Track;
	coverUrl: string | null;
	/** Cover-derived custom properties; the backdrop takes its tint from them. */
	accent?: Record<string, string>;
	/** The Top Nav's window buttons, for a host without a title bar; the view covers them. */
	windowControls?: WindowControls;
	/** Resolves the stored lyrics; null when the host has no lyrics endpoint. */
	loadLyrics?: (trackId: string) => Promise<{ lyrics: string } | null>;
	onClose: () => void;
}) {
	const [loaded, setLoaded] = useState<{
		trackId: string;
		state: LyricsState;
	} | null>(null);
	let state: LyricsState = { status: "loading" };
	if (!loadLyrics) state = { status: "loaded", lyrics: "" };
	else if (loaded?.trackId === track.id) state = loaded.state;
	const lyrics = state.status === "loaded" ? parseLyrics(state.lyrics) : null;
	const rootRef = useRef<HTMLDivElement>(null);
	// The Queue preview's first row lines up with the top of the centered cover.
	const stageRef = useRef<HTMLDivElement>(null);
	const [coverTop, setCoverTop] = useState(0);
	// The cover element is looked up on every measure: AlbumArt swaps its
	// placeholder for the image, and a new title can move the cover.
	const measureCoverTop = useCallback(() => {
		const stage = stageRef.current;
		const cover = stage?.querySelector("section")?.firstElementChild;
		if (!stage || !cover) return;
		setCoverTop(
			cover.getBoundingClientRect().top - stage.getBoundingClientRect().top,
		);
	}, []);
	useLayoutEffect(measureCoverTop);
	useLayoutEffect(() => {
		const stage = stageRef.current;
		if (!stage || typeof ResizeObserver === "undefined") return undefined;
		const observer = new ResizeObserver(measureCoverTop);
		observer.observe(stage);
		return () => observer.disconnect();
	}, [measureCoverTop]);
	const closeRef = useRef(onClose);
	useFocusTrap(rootRef);

	useEffect(() => {
		if (!loadLyrics) return undefined;
		let cancelled = false;
		const trackId = track.id;
		loadLyrics(trackId)
			.then((result) => {
				if (!cancelled)
					setLoaded({
						trackId,
						state: { status: "loaded", lyrics: result?.lyrics ?? "" },
					});
			})
			.catch((error) => {
				console.warn("Failed to load track lyrics", {
					trackId: track.id,
					error,
				});
				if (!cancelled) setLoaded({ trackId, state: { status: "failed" } });
			});
		return () => {
			cancelled = true;
		};
	}, [loadLyrics, track.id]);

	// Closing plays the enter animation in reverse, then unmounts.
	const [closing, setClosing] = useState(false);
	const onCloseRef = useRef(onClose);
	useEffect(() => {
		onCloseRef.current = onClose;
	}, [onClose]);
	useEffect(() => {
		if (!closing) return undefined;
		// The host re-renders on every playback tick with a new onClose, so
		// the timer reads it from a ref instead of restarting.
		const timer = window.setTimeout(() => onCloseRef.current(), EXIT_MS);
		return () => window.clearTimeout(timer);
	}, [closing]);
	useEffect(() => {
		const requestClose = () => {
			const animate = window.matchMedia?.(
				"(prefers-reduced-motion: no-preference)",
			).matches;
			if (animate) setClosing(true);
			else onClose();
		};
		closeRef.current = requestClose;
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") requestClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);

	if (typeof document === "undefined") return null;

	return createPortal(
		<div
			ref={rootRef}
			role="dialog"
			aria-modal="true"
			aria-label="Lyrics"
			tabIndex={-1}
			style={
				{
					"--player-accent": accent["--player-accent"],
					"--lyrics-accent": accent["--player-accent"] ?? "var(--heading)",
					"--cover-top": `${coverTop}px`,
					// Lyrics reach three lines (1.2em text plus py-2 each) past the
					// centered cover and controls. They start beside the cover, scroll
					// on until the last line sits at the bottom, and fade at both edges
					// so no line shows cut in half.
					"--lyrics-line": "calc(1.2em + 1rem)",
					"--lyrics-inset": `max(0px, calc(${coverTop}px - 3.6em - 3rem))`,
					"--lyrics-reach": `min(${coverTop}px, 3.6em + 3rem)`,
				} as CSSProperties
			}
			// Without a cover accent the gradient is invalid and the flat background shows.
			className={cn(
				closing
					? "motion-safe:animate-lyrics-view-exit"
					: "motion-safe:animate-lyrics-view",
				"fixed inset-0 z-fullscreen flex flex-col bg-background bg-linear-to-b from-(--player-accent)/40 to-background to-70% text-foreground outline-none",
			)}
		>
			<header
				data-window-drag-region
				className="flex shrink-0 items-center justify-between gap-4 py-3 pl-6"
			>
				<p className="text-caption text-xs uppercase tracking-widest">Lyrics</p>
				<div className="flex items-center gap-1 pr-2">
					<button
						type="button"
						className="inline-flex size-9 items-center justify-center rounded-full text-foreground hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
						aria-label="Close lyrics"
						title="Close lyrics"
						onClick={() => closeRef.current()}
					>
						<ChevronDown className="size-5" />
					</button>
					{windowControls ? (
						<WindowControlButtons controls={windowControls} />
					) : null}
				</div>
			</header>
			<div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 pb-6 md:flex-row md:overflow-hidden lg:gap-12 lg:px-12 lg:pb-12">
				<div
					ref={stageRef}
					className="flex shrink-0 md:w-2/5 md:overflow-y-auto lg:w-1/3 2xl:w-1/4 scrollbar-hidden"
				>
					<CurrentTrackPanel track={track} coverUrl={coverUrl} />
				</div>
				{/* Lyrics and a Queue preview share the right side; without lyrics
				    the preview sits under a short note, so no column stands empty. */}
				<div
					className={cn(
						"flex min-h-96 min-w-0 flex-1 gap-6 md:min-h-0 lg:gap-12",
						lyrics ? "flex-row" : "flex-col",
					)}
				>
					{lyrics ? (
						<SyncedLyrics lyrics={lyrics} />
					) : (
						<section aria-label="Lyrics text" className="min-w-0">
							<LyricsStatus state={state} />
						</section>
					)}
					<QueuePreview
						className={cn(
							"min-h-0 shrink-0",
							lyrics
								? "hidden w-queue-preview justify-start pt-(--cover-top) xl:flex [&>ol]:-mt-3"
								: "w-queue-preview",
						)}
					/>
				</div>
			</div>
		</div>,
		document.body,
	);
}

/**
 * Lyrics in large type. Synced (LRC) lyrics follow the playhead: the sung
 * line is lit and kept in the middle, sung lines dim, coming ones step down.
 */
function SyncedLyrics({ lyrics }: { lyrics: ParsedLyrics }) {
	const { currentTime, seek } = usePlayback();
	const active = lyrics.synced
		? activeLyricIndex(lyrics.lines, currentTime)
		: -1;
	const scrollerRef = useRef<HTMLElement>(null);
	const activeRef = useRef<HTMLButtonElement>(null);

	useEffect(() => {
		const scroller = scrollerRef.current;
		const line = activeRef.current;
		if (active < 0 || !scroller || !line) return;
		const reduceMotion = window.matchMedia?.(
			"(prefers-reduced-motion: reduce)",
		).matches;
		scroller.scrollTo?.({
			top: line.offsetTop - (scroller.clientHeight - line.clientHeight) / 2,
			behavior: reduceMotion ? "auto" : "smooth",
		});
	}, [active]);

	// Lines inherit text-lyrics from here: tailwind-merge reads it as a color
	// and would drop it next to a line's text-heading.
	return (
		<section
			ref={scrollerRef}
			aria-label="Lyrics text"
			className="relative my-(--lyrics-inset) min-w-0 flex-1 overflow-y-auto pt-(--lyrics-reach) pb-(--lyrics-line) text-lyrics scrollbar-hidden mask-y-from-[calc(100%-var(--lyrics-line))]"
		>
			{lyrics.lines.map((line, index) => {
				const className =
					"block min-h-6 py-2 text-left font-semibold tracking-tight transition-colors duration-300 ease-out";
				const time = line.time;
				if (time === undefined)
					return (
						<p
							// biome-ignore lint/suspicious/noArrayIndexKey: lyric lines have no identity
							key={index}
							className={cn(className, "text-heading")}
						>
							{line.text}
						</p>
					);
				// A synced line jumps the playhead to where it is sung.
				return (
					<button
						// biome-ignore lint/suspicious/noArrayIndexKey: lyric lines have no identity
						key={index}
						type="button"
						ref={index === active ? activeRef : undefined}
						aria-current={index === active ? "true" : undefined}
						className={cn(
							className,
							"hover:text-heading focus-visible:text-heading focus-visible:underline focus-visible:decoration-2 focus-visible:underline-offset-8 focus-visible:outline-none",
							index === active
								? "text-heading"
								: index < active
									? "text-heading/40"
									: "text-heading/70",
						)}
						onClick={() => seek(time)}
					>
						{line.text}
					</button>
				);
			})}
		</section>
	);
}

function LyricsStatus({ state }: { state: LyricsState }): ReactNode {
	if (state.status === "loading") {
		return (
			<p className="px-2 py-6 text-caption text-sm" aria-live="polite">
				Loading lyrics…
			</p>
		);
	}
	if (state.status === "failed") {
		return (
			<div className="max-w-md px-2 py-6" role="alert">
				<p className="font-semibold text-heading text-xl">
					Lyrics could not be loaded
				</p>
				<p className="mt-2 text-caption text-sm">
					The Music Server did not answer. Try again in a moment.
				</p>
			</div>
		);
	}
	return (
		<div className="max-w-md px-2 py-6">
			<p className="font-semibold text-heading text-xl">No lyrics yet</p>
			<p className="mt-2 text-caption text-sm">
				This Track has no lyrics stored on your Music Server. Tag the file with
				lyrics (Picard writes them) and import it again.
			</p>
		</div>
	);
}
