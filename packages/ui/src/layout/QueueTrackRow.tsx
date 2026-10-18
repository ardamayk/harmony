import type { QueueItem } from "@repo/api-client";
import type { Ref } from "react";
import { cn } from "../lib/utils";
import { AlbumArt } from "./AlbumArt";
import { useQueueRowMenu } from "./QueueRowMenuProvider";
import { formatQueueDuration } from "./queue-groups";

type QueueTrackRowProps = {
	ref?: Ref<HTMLLIElement>;
	item: QueueItem;
	number: number;
	isAlbum: boolean;
	state: "played" | "current" | "upcoming";
	isPlaying: boolean;
	coverUrl: string;
	onPlay: () => void;
};

export function QueueTrackRow({
	ref,
	item,
	number,
	isAlbum,
	state,
	isPlaying,
	coverUrl,
	onPlay,
}: QueueTrackRowProps) {
	const renderMenu = useQueueRowMenu();
	const isCurrent = state === "current";
	const row = (
		<li
			ref={ref}
			role="button"
			tabIndex={0}
			aria-current={isCurrent ? "true" : undefined}
			data-queue-state={state}
			onClick={onPlay}
			onKeyDown={(event) => {
				if (
					event.target === event.currentTarget &&
					(event.key === "Enter" || event.key === " ")
				) {
					event.preventDefault();
					onPlay();
				}
			}}
			className={cn(
				"group relative flex cursor-pointer items-center gap-3 rounded-r-sm py-2 pr-3 pl-6 -outline-offset-2 focus-visible:outline-2 focus-visible:outline-player-control-primary",
				// The current row sits on the active step, so hover never darkens it.
				!isCurrent && "hover:bg-muted",
				// Margin plus padding match other rows' `pl-6` / `pr-3`, so number and duration columns stay put.
				isCurrent &&
					"my-1 mr-1 ml-2 rounded-sm bg-shell-active pr-2 pl-4 ring-1 ring-player-control-primary shadow-md shadow-player-control-shadow",
			)}
		>
			{isCurrent ? (
				<QueueEqualizer isPlaying={isPlaying} />
			) : (
				<span className="w-5 shrink-0 text-right text-xs text-caption tabular-nums">
					{number}
				</span>
			)}
			{!isAlbum && !isCurrent && (
				<AlbumArt
					coverUrl={coverUrl}
					title={item.track.title}
					className={cn(
						"size-8 shrink-0 rounded-sm text-xs",
						state === "played" &&
							"opacity-50 group-hover:opacity-100 group-focus-within:opacity-100",
					)}
				/>
			)}
			<span className="min-w-0 flex-1">
				<span
					// Played rows step the title down a text tier instead of fading the
					// row, so their text keeps 4.5:1.
					className={cn(
						"block truncate text-sm leading-5",
						isCurrent
							? "font-semibold text-heading"
							: state === "played"
								? "text-caption group-hover:text-heading group-focus-within:text-heading"
								: "text-heading",
					)}
					title={item.track.title}
				>
					{item.track.title}
				</span>
				{(!isAlbum || isCurrent) && (
					<span
						className={cn(
							"block truncate text-xs leading-4",
							// Caption loses 4.5:1 on the active step.
							isCurrent ? "text-foreground" : "text-caption",
						)}
						title={item.track.artistName}
					>
						{item.track.artistName}
					</span>
				)}
			</span>
			<span
				className={cn(
					"shrink-0 text-xs tabular-nums",
					isCurrent ? "text-foreground" : "text-caption",
				)}
			>
				{formatQueueDuration(item.track.durationMs)}
			</span>
		</li>
	);
	return renderMenu ? renderMenu({ item, children: row }) : row;
}

function QueueEqualizer({ isPlaying }: { isPlaying: boolean }) {
	return (
		<span
			aria-hidden
			className="queue-equalizer flex size-4 shrink-0 items-end justify-center gap-0.5 pb-px text-player-control-primary"
			data-playing={isPlaying}
		>
			<span className="h-1.5 w-0.5 rounded-full bg-current" />
			<span className="h-2.5 w-0.5 rounded-full bg-current" />
			<span className="h-2 w-0.5 rounded-full bg-current" />
		</span>
	);
}
