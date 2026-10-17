import { cn } from "../lib/utils";
import { usePlayback } from "../playback/PlaybackProvider";
import { AlbumArt } from "./AlbumArt";
import { findCurrentQueueIndex, formatQueueDuration } from "./queue-groups";

/** How many Tracks the preview shows on each side of the current one. */
const REACH = 4;
// Rows fade with their distance from the current Track.
const FADE = ["", "opacity-80", "opacity-60", "opacity-40", "opacity-25"];

/**
 * A glance at the Queue for full-screen views: the current Track with the
 * four before and after it, fading out with distance. Clicking a row plays it.
 */
export function QueuePreview({ className }: { className?: string }) {
	const {
		queue,
		currentTrack,
		playbackSource,
		playQueueIndex,
		getAlbumCoverUrl,
	} = usePlayback();
	const current = findCurrentQueueIndex(
		queue,
		playbackSource?.type === "track" ? playbackSource.queueItemId : undefined,
		currentTrack?.id,
	);
	// Without a current item (playback started outside the Queue) the
	// preview simply shows what comes first.
	if (queue.length === 0) return null;
	const start = Math.max(0, current - REACH);
	const rows = queue.slice(start, current + REACH + 1);

	return (
		<section
			aria-label="Queue"
			className={cn("flex flex-col justify-center", className)}
		>
			<h2 className="mb-3 px-3 text-caption text-xs uppercase tracking-widest">
				Queue
			</h2>
			<ol className="flex flex-col gap-1">
				{rows.map((item, offset) => {
					const index = start + offset;
					const distance = Math.abs(index - current);
					return (
						<li key={item.id}>
							<button
								type="button"
								aria-current={distance === 0 ? "true" : undefined}
								className={cn(
									"flex w-full items-center gap-4 rounded-md px-3 py-3 text-left transition duration-200 ease-out hover:bg-heading/5 hover:opacity-100 focus-visible:bg-heading/5 focus-visible:opacity-100 focus-visible:outline-none",
									FADE[distance],
									distance === 0 &&
										"bg-(--lyrics-accent)/20 hover:bg-(--lyrics-accent)/25 focus-visible:bg-(--lyrics-accent)/25",
								)}
								onClick={() =>
									void playQueueIndex(index).catch((error) =>
										console.warn("Failed to play queue track", { error }),
									)
								}
							>
								<AlbumArt
									coverUrl={getAlbumCoverUrl(item.track.albumId, 128)}
									title={item.track.title}
									className="size-10 shrink-0 rounded-sm text-xs xl:size-12"
								/>
								<span className="min-w-0 flex-1">
									<span
										className={cn(
											"block truncate text-base text-heading xl:text-xl",
											distance === 0 && "font-semibold",
										)}
										title={item.track.title}
									>
										{item.track.title}
									</span>
									<span
										className="block truncate text-caption text-sm xl:text-base"
										title={item.track.artistName}
									>
										{item.track.artistName}
									</span>
								</span>
								<span className="shrink-0 text-caption text-sm tabular-nums xl:text-base">
									{formatQueueDuration(item.track.durationMs)}
								</span>
							</button>
						</li>
					);
				})}
			</ol>
		</section>
	);
}
