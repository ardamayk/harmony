import { SkipForward } from "lucide-react";
import { toast } from "sonner";
import { usePlayback } from "../playback/PlaybackProvider";
import { AlbumArt } from "./AlbumArt";
import { findCurrentQueueIndex } from "./queue-groups";

/**
 * The Track that follows the current one, shown beside the now-playing
 * info. Clicking it jumps ahead; hidden on narrow bars and while radio
 * plays, where there is no next Track.
 */
export function UpNextPeek() {
	const {
		queue,
		currentTrack,
		playbackSource,
		playQueueIndex,
		getAlbumCoverUrl,
	} = usePlayback();
	if (!currentTrack || (playbackSource && playbackSource.type !== "track")) {
		return null;
	}
	const queueItemId =
		playbackSource?.type === "track" ? playbackSource.queueItemId : undefined;
	const currentIndex = findCurrentQueueIndex(
		queue,
		queueItemId,
		currentTrack.id,
	);
	const next = currentIndex >= 0 ? queue[currentIndex + 1] : undefined;
	if (!next) return null;
	return (
		<button
			type="button"
			data-player-control
			data-testid="up-next"
			className="@[26rem]/now-playing:flex ml-auto hidden w-[150px] shrink-0 items-center gap-2 rounded-md border border-shell-subtle-border bg-player-pill py-1 pr-2 pl-1 text-left text-player-foreground hover:bg-[rgb(from_var(--player-pill)_r_g_b_/_0.85)]"
			title={`Up next: ${next.track.title}`}
			aria-label={`Up next: ${next.track.title} by ${next.track.artistName}. Play now`}
			onClick={() =>
				void playQueueIndex(currentIndex + 1).catch((error) => {
					console.warn("Failed to play next queue track", {
						itemId: next.id,
						error,
					});
					toast.error("Failed to play next queue track");
				})
			}
		>
			<AlbumArt
				coverUrl={getAlbumCoverUrl(next.track.albumId, 64)}
				title={next.track.title}
				className="size-8 shrink-0 rounded-sm text-xs"
			/>
			<span className="min-w-0">
				<span className="flex items-center gap-1 text-[10px]/[14px] text-caption uppercase tracking-[0.06em]">
					<SkipForward className="size-2.5" aria-hidden />
					Up next
				</span>
				<span className="block truncate text-[11px]/[15px] text-player-title">
					{next.track.title}
				</span>
			</span>
		</button>
	);
}
