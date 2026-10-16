import { Maximize2, X } from "lucide-react";
import { cn } from "../lib/utils";
import { usePlayback } from "../playback/PlaybackProvider";
import { AlbumArt } from "./AlbumArt";
import { PlaybackControls } from "./PlayerBarControls";

/**
 * Contents of the always-on-top mini player window: cover, title, transport
 * and a thin seek bar. The root carries `data-tauri-drag-region` so the
 * undecorated window can be moved by dragging any non-interactive area.
 */
export function MiniPlayerWindow({
	onExpand,
	onClose,
}: {
	/** Bring the main window back (and close this one). */
	onExpand?: () => void;
	onClose?: () => void;
}) {
	const {
		currentTrack,
		currentRadioStation,
		isPlaying,
		currentTime,
		duration,
		bufferedEnd,
		shuffleEnabled,
		repeatMode,
		togglePlay,
		toggleShuffle,
		cycleRepeatMode,
		navigatePrevious,
		navigateNext,
		seek,
		getAlbumCoverUrl,
	} = usePlayback();
	const isRadioPlaying = Boolean(currentRadioStation);
	const title =
		currentTrack?.title ?? currentRadioStation?.name ?? "Nothing playing";
	const subtitle =
		currentTrack?.artistName ??
		(isRadioPlaying ? "Live radio" : "Select a track in the main window");
	const artworkUrl = currentTrack
		? getAlbumCoverUrl(currentTrack.albumId, 128)
		: (currentRadioStation?.faviconUrl ?? null);
	const effectiveDuration =
		duration > 0
			? duration
			: currentTrack?.durationMs
				? currentTrack.durationMs / 1000
				: 0;

	return (
		<div
			data-tauri-drag-region
			data-testid="mini-player-window"
			className="flex h-screen w-screen select-none items-center gap-3 overflow-hidden bg-player px-3 text-player-foreground"
		>
			<AlbumArt
				coverUrl={artworkUrl}
				title={title}
				className="pointer-events-none size-16 shrink-0 rounded-sm text-sm"
			/>
			<div className="flex min-w-0 flex-1 flex-col gap-1">
				<div
					data-tauri-drag-region
					className="flex min-w-0 items-center justify-between gap-2"
				>
					<div data-tauri-drag-region className="min-w-0">
						<p
							data-tauri-drag-region
							className="truncate font-medium text-player-title text-sm"
							title={title}
						>
							{title}
						</p>
						<p
							data-tauri-drag-region
							className="truncate text-player-foreground text-xs"
						>
							{subtitle}
						</p>
					</div>
					<div className="flex shrink-0 items-center gap-1">
						{onExpand ? (
							<button
								type="button"
								data-player-control
								className={cn(
									"inline-flex size-6 relative after:absolute after:-inset-1 after:content-[''] items-center justify-center rounded-sm text-player-foreground hover:text-player-control-primary",
								)}
								aria-label="Open main window"
								onClick={onExpand}
							>
								<Maximize2 className="size-3.5" />
							</button>
						) : null}
						{onClose ? (
							<button
								type="button"
								data-player-control
								className="inline-flex size-6 relative after:absolute after:-inset-1 after:content-[''] items-center justify-center rounded-sm text-player-foreground hover:text-player-control-primary"
								aria-label="Close mini player"
								onClick={onClose}
							>
								<X className="size-3.5" />
							</button>
						) : null}
					</div>
				</div>
				<PlaybackControls
					isRadioPlaying={isRadioPlaying}
					isPlaying={isPlaying}
					hasPlayableSource={Boolean(currentTrack || currentRadioStation)}
					hasCurrentTrack={Boolean(currentTrack)}
					currentTime={currentTime}
					effectiveDuration={effectiveDuration}
					bufferedEnd={bufferedEnd}
					showHoverTimestamp={false}
					shuffleEnabled={shuffleEnabled}
					repeatMode={repeatMode}
					onTogglePlay={togglePlay}
					onToggleShuffle={toggleShuffle}
					onCycleRepeatMode={cycleRepeatMode}
					onPrevious={navigatePrevious}
					onNext={navigateNext}
					onSeek={seek}
				/>
			</div>
		</div>
	);
}
