import type { Track } from "@repo/api-client";
import { ChevronDown } from "lucide-react";
import {
	type CSSProperties,
	type ReactNode,
	useEffect,
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
import { QueuePanel } from "./QueuePanel";

/**
 * Full-screen listening view with current Track, stored lyrics, and shared Queue.
 */
type LyricsState =
	| { status: "loading" }
	| { status: "loaded"; lyrics: string }
	| { status: "failed" };

export function LyricsOverlay({
	track,
	coverUrl,
	accent = {},
	loadLyrics,
	onClose,
}: {
	track: Track;
	coverUrl: string | null;
	/** Cover-derived custom properties; the backdrop takes its tint from them. */
	accent?: Record<string, string>;
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

	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
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
			style={{ "--player-accent": accent["--player-accent"] } as CSSProperties}
			// Without a cover accent the gradient is invalid and the flat background shows.
			className="motion-safe:animate-lyrics-view fixed inset-0 z-fullscreen flex flex-col bg-background bg-linear-to-b from-(--player-accent)/40 to-background to-70% text-foreground outline-none"
		>
			<header className="flex shrink-0 items-center justify-between gap-4 px-6 py-3">
				<p className="text-caption text-xs uppercase tracking-widest">Lyrics</p>
				<button
					type="button"
					className="inline-flex size-9 items-center justify-center rounded-full text-foreground hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
					aria-label="Close lyrics"
					title="Close lyrics"
					onClick={onClose}
				>
					<ChevronDown className="size-5" />
				</button>
			</header>
			<div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 pb-6 md:flex-row md:overflow-hidden lg:gap-12 lg:px-12 lg:pb-12">
				<div className="flex shrink-0 md:w-2/5 md:overflow-y-auto xl:w-1/3 scrollbar-hidden">
					<CurrentTrackPanel track={track} coverUrl={coverUrl} />
				</div>
				{/* Lyrics and the Queue share the right side; without lyrics the
				    Queue takes it whole under a short note, so no column stands empty. */}
				<div
					className={cn(
						"flex min-h-96 min-w-0 flex-1 gap-6 md:min-h-0",
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
					<aside
						className={cn(
							"min-h-0 overflow-hidden rounded-lg border border-border bg-queue text-queue-foreground",
							lyrics ? "hidden w-lyrics-aside shrink-0 lg:block" : "flex-1",
						)}
					>
						<QueuePanel embedded />
					</aside>
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
			className={cn(
				"relative min-w-0 flex-1 overflow-y-auto text-lyrics scrollbar-hidden",
				lyrics.synced ? "py-lyrics-edge" : "py-12",
			)}
		>
			{lyrics.lines.map((line, index) => {
				const className =
					"block min-h-6 max-w-3xl py-2 text-left font-semibold tracking-tight transition-colors duration-300 ease-out";
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
							"-mx-2 rounded-md px-2 hover:bg-heading/5 hover:text-heading focus-visible:bg-heading/5 focus-visible:text-heading focus-visible:outline-none",
							index === active
								? "text-heading"
								: index < active
									? "text-heading/40"
									: "text-heading/60",
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
