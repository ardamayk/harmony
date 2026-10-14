import type { Track } from "@repo/api-client";
import { ChevronDown } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useFocusTrap } from "../lib/use-focus-trap";
import { AlbumArt } from "./AlbumArt";
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
	loadLyrics,
	onClose,
}: {
	track: Track;
	coverUrl: string | null;
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
			className="lyrics-view-enter fixed inset-0 z-[60] flex flex-col bg-background text-foreground outline-none"
		>
			<header className="flex shrink-0 items-center justify-between gap-4 border-border border-b px-6 py-3">
				<div className="flex min-w-0 items-center gap-3">
					<AlbumArt
						coverUrl={coverUrl}
						title={track.title}
						className="size-12 shrink-0 rounded-md text-sm"
					/>
					<div className="min-w-0">
						<p className="truncate font-semibold text-heading text-sm">
							{track.title}
						</p>
						<p className="truncate text-caption text-xs">
							{track.artistName}
							{track.albumTitle ? ` · ${track.albumTitle}` : ""}
						</p>
					</div>
				</div>
				<button
					type="button"
					className="inline-flex size-9 items-center justify-center rounded-full text-foreground hover:bg-muted"
					aria-label="Close lyrics"
					onClick={onClose}
				>
					<ChevronDown className="size-5" />
				</button>
			</header>
			<div className="flex min-h-0 flex-1 flex-col overflow-y-auto md:flex-row md:overflow-hidden">
				<div className="shrink-0 border-border border-b md:w-[34%] md:overflow-y-auto md:border-r md:border-b-0">
					<CurrentTrackPanel track={track} coverUrl={coverUrl} />
				</div>
				<section
					aria-label="Lyrics text"
					className="flex min-h-64 min-w-0 flex-1 flex-col md:overflow-y-auto px-8 py-10 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
				>
					<LyricsBody state={state} />
				</section>
				<aside className="hidden w-[clamp(14rem,22vw,18rem)] shrink-0 border-border border-l bg-queue text-queue-foreground md:block">
					<QueuePanel embedded />
				</aside>
			</div>
		</div>,
		document.body,
	);
}

function LyricsBody({ state }: { state: LyricsState }): ReactNode {
	if (state.status === "loading") {
		return (
			<p className="m-auto text-caption text-sm" aria-live="polite">
				Loading lyrics…
			</p>
		);
	}
	if (state.status === "failed") {
		return (
			<div className="m-auto max-w-md text-center" role="alert">
				<p className="font-semibold text-heading text-lg">
					Lyrics could not be loaded
				</p>
				<p className="mt-2 text-caption text-sm">
					The Music Server did not answer. Try again in a moment.
				</p>
			</div>
		);
	}
	const lines = state.lyrics
		.split(/\r?\n/)
		.map((line) => line.trimEnd())
		.filter((line, index, all) => line !== "" || all[index - 1] !== "");
	if (lines.every((line) => line === "")) {
		return (
			<div className="m-auto max-w-md text-center">
				<p className="font-semibold text-heading text-lg">No lyrics yet</p>
				<p className="mt-2 text-caption text-sm">
					This Track has no lyrics stored on your Music Server. Tag the file
					with lyrics (Picard writes them) and import it again.
				</p>
			</div>
		);
	}
	return (
		<div className="mx-auto w-full max-w-2xl">
			{lines.map((line, index) => (
				<p
					// biome-ignore lint/suspicious/noArrayIndexKey: lyric lines have no identity
					key={index}
					className="min-h-[1.5em] font-semibold text-2xl text-heading leading-relaxed"
				>
					{line}
				</p>
			))}
		</div>
	);
}
