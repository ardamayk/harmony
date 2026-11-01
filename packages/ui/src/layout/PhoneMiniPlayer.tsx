import type { CSSProperties } from "react";
import { AlbumArt } from "./AlbumArt";
import { PrimaryPlaybackButton } from "./PlayerBarControls";

/**
 * The phone's Player Bar: cover, title, artist, play/pause and a thin progress
 * line. The whole row opens Now Playing, which only a Track has
 * (`onOpen` absent for radio and External Tracks, as on the desktop cover).
 */
export function PhoneMiniPlayer({
	coverUrl,
	title,
	subtitle,
	isPlaying,
	progress,
	onOpen,
	onTogglePlay,
}: {
	coverUrl: string | null;
	title: string;
	subtitle: string;
	isPlaying: boolean;
	/** 0..1; null hides the line (live radio has no length). */
	progress: number | null;
	onOpen?: () => void;
	onTogglePlay: () => void;
}) {
	const details = (
		<>
			<AlbumArt
				key={coverUrl ?? "none"}
				coverUrl={coverUrl}
				title={title}
				className="size-10 shrink-0 rounded-sm border border-shell-subtle-border bg-player-artwork text-sm"
			/>
			<span className="min-w-0 flex-1">
				<span
					className="block truncate font-medium text-player-title text-sm"
					title={title}
				>
					{title}
				</span>
				<span
					className="block truncate text-player-foreground text-xs"
					title={subtitle}
				>
					{subtitle}
				</span>
			</span>
		</>
	);
	const detailsClass = "flex min-h-11 min-w-0 flex-1 items-center gap-3";
	return (
		<section
			data-mini-player
			aria-label="Now playing"
			className="relative flex h-14 items-center gap-3 border-player-border border-t px-(--shell-inset,2rem)"
		>
			{progress === null ? null : (
				<div
					aria-hidden
					style={{ "--mini-progress": `${progress * 100}%` } as CSSProperties}
					className="absolute top-0 left-0 h-0.5 w-(--mini-progress) bg-player-live-progress"
				/>
			)}
			{onOpen ? (
				<button
					type="button"
					aria-label={`Open now playing: ${title}`}
					className={`${detailsClass} rounded-md text-left transition hover:bg-muted focus-visible:bg-muted focus-visible:outline-none`}
					onClick={onOpen}
				>
					{details}
				</button>
			) : (
				<div className={detailsClass}>{details}</div>
			)}
			<PrimaryPlaybackButton
				isPlaying={isPlaying}
				disabled={false}
				onClick={onTogglePlay}
			/>
		</section>
	);
}
