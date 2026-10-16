import type { Track } from "@repo/api-client";
import { ChevronDown, MicVocal } from "lucide-react";
import { type CSSProperties, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useFocusTrap } from "../lib/use-focus-trap";
import { cn } from "../lib/utils";
import { CurrentTrackPanel } from "./CurrentTrackPanel";
import { QueuePanel } from "./QueuePanel";

/**
 * Full-screen "Now Playing" view: pointer-responsive artwork, the
 * transport and the Queue. Sibling of the Lyrics view and opened from the
 * cover in the Player Bar.
 */
export function NowPlayingView({
	track,
	coverUrl,
	accentStyle,
	onOpenLyrics,
	onClose,
}: {
	track: Track;
	coverUrl: string | null;
	/** Cover-derived custom properties, forwarded so the view matches the bar. */
	accentStyle?: CSSProperties;
	onOpenLyrics?: () => void;
	onClose: () => void;
}) {
	const rootRef = useRef<HTMLDivElement>(null);
	useFocusTrap(rootRef);

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
			aria-label="Now playing"
			tabIndex={-1}
			data-now-playing-surface
			style={accentStyle}
			className="lyrics-view-enter fixed inset-0 z-[60] flex flex-col overflow-hidden bg-background text-foreground outline-none"
		>
			<header className="relative flex shrink-0 items-center justify-between gap-4 px-6 py-3">
				<p className="text-caption text-xs uppercase tracking-[0.2em]">
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
					/>
					{onOpenLyrics ? (
						<button
							type="button"
							className={cn(
								"inline-flex items-center gap-2 rounded-full border border-border px-4 py-2 text-sm hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
							)}
							onClick={onOpenLyrics}
						>
							<MicVocal className="size-4" aria-hidden />
							Lyrics
						</button>
					) : null}
				</div>
				<aside className="hidden w-[22rem] shrink-0 border-border border-l bg-background text-queue-foreground md:block">
					<QueuePanel embedded />
				</aside>
			</div>
		</div>,
		document.body,
	);
}
