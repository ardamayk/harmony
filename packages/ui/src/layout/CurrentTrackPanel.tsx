import type { Track } from "@repo/api-client";
import { usePlayback } from "../playback/PlaybackProvider";
import { AlbumArt } from "./AlbumArt";
import { PlaybackControls } from "./PlayerBarControls";
import { TiltingArtwork } from "./TiltingArtwork";

export function CurrentTrackPanel({
	track,
	coverUrl,
	isArtworkFocused = false,
}: {
	track: Track;
	coverUrl: string | null;
	isArtworkFocused?: boolean;
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
			className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-6 p-6"
		>
			{isArtworkFocused ? (
				<TiltingArtwork coverUrl={coverUrl} title={track.title} />
			) : (
				<AlbumArt
					coverUrl={coverUrl}
					title={track.title}
					className="aspect-square w-full max-w-sm shrink-0 rounded-lg text-3xl shadow-xl"
				/>
			)}
			<div className="w-full max-w-xl text-center">
				<h1 className="truncate font-semibold text-2xl text-heading">
					{track.title}
				</h1>
				<p className="mt-1 truncate text-base text-caption">
					{track.artistName}
					{track.albumTitle ? ` · ${track.albumTitle}` : ""}
				</p>
			</div>
			<div className="w-full max-w-xl [--player-control-primary:var(--primary)] [--player-control-primary-foreground:var(--primary-foreground)] [--player-foreground:var(--foreground)]">
				<PlaybackControls
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
