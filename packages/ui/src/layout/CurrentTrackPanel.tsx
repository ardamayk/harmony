import type { Track } from "@repo/api-client";
import { cn } from "../lib/utils";
import { usePlayback } from "../playback/PlaybackProvider";
import { AlbumArt } from "./AlbumArt";
import { PlaybackControls } from "./PlayerBarControls";
import { TiltingArtwork } from "./TiltingArtwork";

export function CurrentTrackPanel({
	track,
	coverUrl,
	isArtworkFocused = false,
	className,
}: {
	track: Track;
	coverUrl: string | null;
	isArtworkFocused?: boolean;
	className?: string;
}) {
	const {
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
	} = usePlayback();
	const effectiveDuration =
		duration > 0
			? duration
			: track.durationMs > 0
				? track.durationMs / 1000
				: 0;

	return (
		<section
			aria-label="Current track"
			className={cn(
				"flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-8 p-6",
				className,
			)}
		>
			{isArtworkFocused ? (
				<TiltingArtwork coverUrl={coverUrl} title={track.title} />
			) : (
				<AlbumArt
					coverUrl={coverUrl}
					title={track.title}
					className="aspect-square w-artwork-stage shrink-0 rounded-md text-3xl shadow-xl"
				/>
			)}
			<div className="w-full max-w-xl text-center">
				<h1
					className="truncate font-semibold text-3xl text-heading"
					title={track.title}
				>
					{track.title}
				</h1>
				<p className="mt-2 truncate text-caption text-xl">
					{track.artistName}
					{track.albumTitle ? ` · ${track.albumTitle}` : ""}
				</p>
			</div>
			<div className="w-full max-w-md player-neutral 2xl:max-w-xl">
				<PlaybackControls
					stage
					isRadioPlaying={false}
					isPlaying={isPlaying}
					hasPlayableSource
					hasCurrentTrack
					currentTime={currentTime}
					effectiveDuration={effectiveDuration}
					bufferedEnd={bufferedEnd}
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
		</section>
	);
}
