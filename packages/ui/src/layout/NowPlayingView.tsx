import type { Track } from "@repo/api-client";
import { ChevronDown, Heart, ListMusic, MicVocal } from "lucide-react";
import { type CSSProperties, useEffect, useRef, useState } from "react";
import { createPortal, flushSync } from "react-dom";
import { useFocusTrap } from "../lib/use-focus-trap";
import { cn } from "../lib/utils";
import { CurrentTrackPanel } from "./CurrentTrackPanel";
import { QueuePanel } from "./QueuePanel";
import { usePhoneShell } from "./use-phone-shell";

const VIEW_ACTION_CLASS =
	"inline-flex items-center gap-2 rounded-full border border-border px-4 py-2 text-sm hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40";

/**
 * Full-screen "Now Playing" view: pointer-responsive artwork, the
 * transport and the Queue. Sibling of the Lyrics view and opened from the
 * cover in the Player Bar. On a phone the Queue takes the whole view instead
 * of an aside, and the view adds favorite and Queue buttons beside Lyrics.
 */
export function NowPlayingView({
	track,
	coverUrl,
	accent = {},
	onOpenLyrics,
	isFavorite = false,
	onToggleFavorite,
	onClose,
}: {
	track: Track;
	coverUrl: string | null;
	/** Cover-derived custom properties, forwarded so the view matches the bar. */
	accent?: Record<string, string>;
	onOpenLyrics?: () => void;
	isFavorite?: boolean;
	/** The phone's favorite button; the bar has its own elsewhere. */
	onToggleFavorite?: () => void;
	onClose: () => void;
}) {
	const isPhone = usePhoneShell();
	const [queueOpen, setQueueOpen] = useState(false);
	const queueButtonRef = useRef<HTMLButtonElement>(null);
	const rootRef = useRef<HTMLDivElement>(null);
	useFocusTrap(rootRef);

	const openQueue = () => {
		flushSync(() => setQueueOpen(true));
		rootRef.current?.focus();
	};
	const closeQueue = () => {
		flushSync(() => setQueueOpen(false));
		queueButtonRef.current?.focus();
	};

	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key !== "Escape") return;
			if (queueOpen) closeQueue();
			else onClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose, queueOpen]);

	if (typeof document === "undefined") return null;

	const lyricsButton = onOpenLyrics ? (
		<button type="button" className={VIEW_ACTION_CLASS} onClick={onOpenLyrics}>
			<MicVocal className="size-4" aria-hidden />
			Lyrics
		</button>
	) : null;

	return createPortal(
		<div
			ref={rootRef}
			role="dialog"
			aria-modal="true"
			aria-label="Now playing"
			tabIndex={-1}
			data-now-playing-surface
			style={
				{
					"--player-accent": accent["--player-accent"],
					"--player-accent-foreground": accent["--player-accent-foreground"],
					"--player-control-primary": accent["--player-control-primary"],
					"--player-control-primary-foreground":
						accent["--player-control-primary-foreground"],
					"--player-live-progress": accent["--player-live-progress"],
					"--player-control-shadow": accent["--player-control-shadow"],
				} as CSSProperties
			}
			className={cn(
				"motion-safe:animate-lyrics-view fixed inset-0 z-fullscreen flex flex-col overflow-hidden bg-background text-foreground outline-none",
				isPhone && "pt-safe-top pb-safe-bottom",
			)}
		>
			{queueOpen ? (
				<div className="min-h-0 flex-1 text-queue-foreground">
					<QueuePanel embedded onClose={closeQueue} />
				</div>
			) : (
				<>
					<header className="relative flex shrink-0 items-center justify-between gap-4 px-6 py-3">
						<p className="text-caption text-xs uppercase tracking-widest">
							Now playing
						</p>
						<button
							type="button"
							className="inline-flex size-9 items-center justify-center rounded-full text-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
							aria-label="Close now playing"
							onClick={onClose}
						>
							<ChevronDown className="size-5" />
						</button>
					</header>
					<div className="relative flex min-h-0 flex-1">
						<div className="flex min-h-0 min-w-0 flex-1 flex-col items-center overflow-y-auto pb-4">
							<CurrentTrackPanel
								track={track}
								coverUrl={coverUrl}
								isArtworkFocused
								// Full width, and scrolls instead of squeezing on short screens.
								className={
									isPhone ? "w-full shrink-0 basis-auto gap-4" : undefined
								}
							/>
							{isPhone ? (
								<div className="flex items-center gap-2">
									{onToggleFavorite ? (
										<button
											type="button"
											aria-label={
												isFavorite
													? "Remove from favorites"
													: "Add to favorites"
											}
											title={
												isFavorite
													? "Remove from favorites"
													: "Add to favorites"
											}
											aria-pressed={isFavorite}
											className={cn(
												"inline-flex size-11 items-center justify-center rounded-full border border-border hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
												isFavorite && "text-player-control-primary",
											)}
											onClick={onToggleFavorite}
										>
											<Heart
												className={cn("size-4", isFavorite && "fill-current")}
											/>
										</button>
									) : null}
									{lyricsButton}
									<button
										ref={queueButtonRef}
										type="button"
										className={VIEW_ACTION_CLASS}
										onClick={openQueue}
									>
										<ListMusic className="size-4" aria-hidden />
										Queue
									</button>
								</div>
							) : (
								lyricsButton
							)}
						</div>
						{isPhone ? null : (
							<aside className="hidden w-88 shrink-0 border-border border-l bg-background text-queue-foreground md:block">
								<QueuePanel embedded />
							</aside>
						)}
					</div>
				</>
			)}
		</div>,
		document.body,
	);
}
