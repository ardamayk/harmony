import type { QueueItem } from "@repo/api-client";
import { Fragment, type Ref } from "react";
import { cn } from "../lib/utils";
import {
	getTrackArtistCredits,
	getTrackArtistName,
} from "../playback/track-details";
import { AlbumArt } from "./AlbumArt";
import { ExplicitBadge } from "./ExplicitBadge";
import {
	queueArtistHref,
	useQueueLinkClick,
	useQueueRowMenu,
} from "./QueueRowMenuProvider";
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
	const linkClick = useQueueLinkClick();
	// Every credited Artist, so a featured singer shows too; each one with a
	// library identity links to their page.
	const artistName = getTrackArtistName(item.track);
	const credits = item.track.artists ?? [];
	// Legacy string credits have no page, so they stay plain text.
	const linked = new Set(getTrackArtistCredits(item.track));
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
				"group relative flex cursor-pointer items-center gap-3 rounded-r-sm py-2 pr-3 pl-3 -outline-offset-2 focus-visible:outline-2 focus-visible:outline-player-control-primary",
				// The current row sits on the active step, so hover never darkens it.
				!isCurrent && "hover:bg-muted",
				// As wide as the group's header, so the two blocks share their edges.
				isCurrent && "my-1 rounded-sm bg-neutral-active",
			)}
		>
			{isCurrent ? (
				<span className="flex w-5 shrink-0 justify-end">
					<QueueEqualizer isPlaying={isPlaying} />
				</span>
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
				<span className="flex min-w-0 items-center gap-2">
					<span
						// Played rows step the title down a text tier instead of fading the
						// row, so their text keeps 4.5:1.
						className={cn(
							"min-w-0 truncate text-sm leading-5",
							// The playing title takes the accent: the one row it marks.
							isCurrent
								? "font-semibold text-shell-active-foreground"
								: state === "played"
									? "text-caption group-hover:text-heading group-focus-within:text-heading"
									: "text-foreground group-hover:text-heading group-focus-within:text-heading",
						)}
						title={item.track.title}
					>
						{item.track.title}
					</span>
					{item.track.explicit ? <ExplicitBadge /> : null}
				</span>
				{(!isAlbum || isCurrent) && (
					<span
						className={cn(
							"block truncate text-xs leading-4",
							// Caption loses 4.5:1 on the active step.
							isCurrent ? "text-foreground" : "text-caption",
						)}
						title={artistName}
					>
						{credits.length > 0
							? credits.map((credit, index) => (
									<Fragment key={`${credit.id}-${index}`}>
										{index > 0 ? ", " : null}
										{linked.has(credit) ? (
											<a
												href={queueArtistHref(credit.id)}
												onClick={linkClick(queueArtistHref(credit.id))}
												className="rounded-sm hover:text-heading hover:underline"
											>
												{credit.name}
											</a>
										) : (
											credit.name
										)}
									</Fragment>
								))
							: artistName}
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

export function QueueEqualizer({
	isPlaying,
	className = "text-player-control-primary",
}: {
	isPlaying: boolean;
	className?: string;
}) {
	return (
		<span
			aria-hidden
			className={cn(
				"queue-equalizer flex size-4 shrink-0 items-end justify-center gap-0.5 pb-px",
				className,
			)}
			data-playing={isPlaying}
		>
			<span className="h-1.5 w-0.5 rounded-full bg-current" />
			<span className="h-2.5 w-0.5 rounded-full bg-current" />
			<span className="h-2 w-0.5 rounded-full bg-current" />
		</span>
	);
}
