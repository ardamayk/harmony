import type { QueueItem } from "@repo/api-client";
import type { ReactNode, Ref } from "react";
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
	const renderRow = (menuTrigger: ReactNode) => (
		<li
			ref={ref}
			role="button"
			tabIndex={0}
			aria-current={isCurrent ? "true" : undefined}
			data-queue-state={state}
			onClick={(event) => {
				if (!(event.target as HTMLElement).closest("[data-queue-menu-trigger]"))
					onPlay();
			}}
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
				"group relative flex cursor-pointer items-center gap-3 rounded-r-sm py-2 pr-3 pl-6 outline-offset-[-2px] hover:bg-[rgb(from_var(--player-pill)_r_g_b_/_0.6)] focus-visible:outline-2 focus-visible:outline-player-control-primary",
				state === "played" &&
					"opacity-50 hover:opacity-100 focus-within:opacity-100",
				isCurrent &&
					"my-1 mr-2 ml-2 rounded-sm bg-[rgb(from_var(--player-pill)_r_g_b_/_0.7)] py-2 pr-3 pl-4 shadow-[0_0_0_1px_var(--player-control-primary),0_6px_18px_-6px_var(--player-control-shadow)]",
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
					className="size-8 shrink-0 rounded-sm text-xs"
				/>
			)}
			<span className="min-w-0 flex-1">
				<span
					className={cn(
						"block truncate text-sm text-heading leading-5",
						isCurrent && "font-semibold text-player-control-primary",
					)}
					title={item.track.title}
				>
					{item.track.title}
				</span>
				{(!isAlbum || isCurrent) && (
					<span
						className="block truncate text-xs text-caption leading-4"
						title={item.track.artistName}
					>
						{item.track.artistName}
					</span>
				)}
			</span>
			<span
				className={cn(
					"shrink-0 text-xs text-caption tabular-nums",
					renderMenu && "group-hover:opacity-0 group-focus-within:opacity-0",
				)}
			>
				{formatQueueDuration(item.track.durationMs)}
			</span>
			{menuTrigger}
		</li>
	);
	return renderMenu
		? renderMenu({ item, children: renderRow })
		: renderRow(null);
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
