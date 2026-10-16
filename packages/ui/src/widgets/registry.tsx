import type { ReactNode } from "react";
import { usePlayback } from "../playback/PlaybackProvider";
import type { WidgetDefinition } from "./types";

function formatDuration(ms: number): string {
	if (!ms || ms < 0) return "0:00";
	const total = Math.floor(ms / 1000);
	const m = Math.floor(total / 60);
	const s = total % 60;
	return `${m}:${s.toString().padStart(2, "0")}`;
}

function NowPlayingWidget() {
	const { currentTrack, isPlaying, currentTime, duration, togglePlay } =
		usePlayback();

	if (!currentTrack) {
		return (
			<div className="min-w-0 rounded-md border border-border p-4 text-foreground text-sm">
				Nothing playing
			</div>
		);
	}

	const progress =
		duration > 0 ? Math.min(100, (currentTime / duration) * 100) : 0;

	return (
		<div className="min-w-0 space-y-2 rounded-md border border-border p-4">
			<p
				className="truncate font-medium text-heading text-sm"
				title={currentTrack.title}
			>
				{currentTrack.title}
			</p>
			<p
				className="truncate text-foreground text-xs"
				title={currentTrack.artistName}
			>
				{currentTrack.artistName}
			</p>
			<div className="h-1 overflow-hidden rounded-full bg-muted">
				<div
					className="h-full bg-primary transition-all"
					style={{ width: `${progress}%` }}
				/>
			</div>
			<div className="flex min-w-0 items-center justify-between gap-2 text-caption text-xs">
				<span className="shrink-0">{formatDuration(currentTime * 1000)}</span>
				<button
					type="button"
					className="min-w-0 truncate rounded-sm px-2 py-1 hover:bg-muted"
					onClick={togglePlay}
				>
					{isPlaying ? "Pause" : "Play"}
				</button>
				<span className="shrink-0">{formatDuration(duration * 1000)}</span>
			</div>
		</div>
	);
}

function QueueWidget() {
	const { queue, currentTrack, playQueueIndex, removeFromQueue } =
		usePlayback();

	if (queue.length === 0) {
		return (
			<div className="min-w-0 rounded-md border border-border p-4 text-foreground text-sm">
				Queue is empty
			</div>
		);
	}

	return (
		<ul className="min-w-0 max-h-64 space-y-1 overflow-y-auto rounded-md border border-border p-2 text-sm">
			{queue.map((item, index) => (
				<li
					key={item.id}
					className="flex min-w-0 items-center gap-2 rounded-sm px-2 py-1 hover:bg-muted"
				>
					<button
						type="button"
						className="min-w-0 flex-1 truncate text-left"
						onClick={() => void playQueueIndex(index)}
					>
						<span
							className={
								item.track.id === currentTrack?.id
									? "block truncate font-medium text-heading"
									: "block truncate text-foreground"
							}
						>
							{item.track.title}
						</span>
						<span className="block truncate text-foreground text-xs">
							{item.track.artistName}
						</span>
					</button>
					<button
						type="button"
						className="shrink-0 text-caption text-xs hover:text-foreground"
						onClick={() => void removeFromQueue(item.id)}
						aria-label="Remove from queue"
					>
						×
					</button>
				</li>
			))}
		</ul>
	);
}

function DiscoverWidget() {
	return (
		<div className="min-w-0 rounded-md border border-dashed border-border p-4 text-caption text-sm">
			Discover (coming soon)
		</div>
	);
}

export const widgetRegistry: WidgetDefinition[] = [
	{ id: "now-playing", title: "Now Playing", component: NowPlayingWidget },
	{ id: "queue", title: "Queue", component: QueueWidget },
	{ id: "discover", title: "Discover", component: DiscoverWidget },
];

/** The registered widget for an id, or null when the id is unknown. */
export function renderWidget(id: string): ReactNode {
	const Widget = widgetRegistry.find((widget) => widget.id === id)?.component;
	return Widget ? <Widget /> : null;
}
