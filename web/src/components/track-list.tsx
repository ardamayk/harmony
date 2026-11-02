import type { QueueItemSource, Track } from "@repo/api-client";
import {
	buildTrackDetailRows,
	ExplicitBadge,
	QueueEqualizer,
	RowMenuButton,
	toast,
	usePhoneShell,
	usePlayback,
} from "@repo/ui";
import { useHiddenFromGuest } from "@repo/ui/guest-access";
import {
	Clock,
	Download,
	Heart,
	Info,
	ListMinus,
	ListPlus,
	Replace,
	SkipForward,
	Trash2,
	X,
} from "lucide-react";
import { useRef, useState } from "react";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import { Button } from "#/components/ui/button";
import {
	Dialog,
	DialogClose,
	DialogContent,
	DialogTitle,
} from "#/components/ui/dialog";
import { useFavoriteTracks } from "#/hooks/use-favorite-tracks";
import { useReturnFocus } from "#/hooks/use-return-focus";
import { useServerCapability } from "#/hooks/use-server-capability";
import { useTrackDeletionFlow } from "#/hooks/use-track-deletion-flow";
import { useTrackReplacementFlow } from "#/hooks/use-track-replacement-flow";
import { getTrackArtistName } from "#/lib/library-display";
import { cn } from "#/lib/utils";
import { TrackDeletionDialog } from "./track-deletion-dialog";
import { TrackReplacementDialog } from "./track-replacement-dialog";

const MANAGED_TRACK_DELETION_CAPABILITY = "managed-track-deletion.v1";
const MANAGED_TRACK_REPLACEMENT_CAPABILITY = "managed-track-replacement.v1";

function formatDuration(ms: number): string {
	if (!ms || ms < 0) return "0:00";
	const total = Math.floor(ms / 1000);
	const m = Math.floor(total / 60);
	const s = total % 60;
	return `${m}:${s.toString().padStart(2, "0")}`;
}

export function TrackList({
	tracks,
	contextTracks,
	source,
	albumId,
	playMode = "single",
	showFavorite = false,
	showDelete = true,
	compact = false,
	numbering = "track",
	onRemoveTrack,
	onDeleteTrackSuccess,
	onReplaceTrackSuccess,
	removeLabel = "Remove",
	playCounts,
	phoneDownloads = false,
}: {
	tracks: Track[];
	contextTracks?: Track[];
	source?: QueueItemSource;
	albumId?: string;
	playMode?: "single" | "double";
	showFavorite?: boolean;
	showMeta?: boolean;
	showDelete?: boolean;
	compact?: boolean;
	numbering?: "track" | "list";
	onRemoveTrack?: (track: Track) => void;
	onDeleteTrackSuccess?: (track: Track) => void;
	onReplaceTrackSuccess?: (track: Track) => void;
	removeLabel?: string;
	/** Last.fm play counts by Track id; the column shows only when there are any. */
	playCounts?: Map<string, number>;
	/**
	 * On the phone: no header row, and a download button where the duration
	 * sits. Downloads are not built yet, so the button only says so.
	 */
	phoneDownloads?: boolean;
}) {
	const {
		playTrack,
		addToQueue,
		playNext,
		currentTrack,
		isPlaying: isPlaybackRunning,
		getAlbumCoverUrl,
	} = usePlayback();
	const { isFavorite, toggleFavorite } = useFavoriteTracks();
	const trackDeletion = useTrackDeletionFlow(onDeleteTrackSuccess);
	const hasDeletionCapability = useServerCapability(
		MANAGED_TRACK_DELETION_CAPABILITY,
	);
	const trackReplacement = useTrackReplacementFlow(onReplaceTrackSuccess);
	const hasReplacementCapability = useServerCapability(
		MANAGED_TRACK_REPLACEMENT_CAPABILITY,
	);
	const hideDeletion = useHiddenFromGuest("track-deletion");
	const hideReplacement = useHiddenFromGuest("track-replacement");
	const hideQueueEditing = useHiddenFromGuest("queue-editing");
	const hideFavorites = useHiddenFromGuest("favorites");
	const canFavorite = showFavorite && !hideFavorites;
	const isPhone = usePhoneShell();
	// A touch has no double click, so a tap always plays.
	const tapPlays = playMode === "single" || isPhone;
	const [detailsTrack, setDetailsTrack] = useState<Track | null>(null);
	// Context menu items unmount with the menu, so dialogs opened from them
	// return focus to the originating row (or the table once a row is gone).
	const rowRefs = useRef(new Map<string, HTMLTableRowElement>());
	const tableRef = useRef<HTMLTableElement>(null);
	const returnFocus = useReturnFocus();
	const captureRowFocus = (trackId: string) =>
		returnFocus.capture(rowRefs.current.get(trackId));
	const restoreRowFocus = () => returnFocus.restore(tableRef.current);

	// Selecting a Track replaces the Queue with the context from that Track
	// onward, so the previous Queue is gone and no earlier Track in the list
	// is presented as already played.
	const handlePlay = (track: Track) => {
		const context = contextTracks ?? tracks;
		const startIndex = context.findIndex((t) => t.id === track.id);
		const queueTrackIds = context
			.slice(startIndex < 0 ? 0 : startIndex)
			.map((t) => t.id);
		void playTrack(track.id, queueTrackIds, ...(source ? [source] : []));
	};

	const handleQueueAction = async (
		trackId: string,
		action: (trackId: string) => Promise<void>,
		message: string,
	) => {
		try {
			await action(trackId);
		} catch (error) {
			console.warn(message, { trackId, error });
			toast.error(message);
		}
	};

	const rowPadding = compact ? "px-3 py-2" : "px-3 py-3";
	const favoritePadding = compact ? "px-2 py-2" : "px-2 py-3";
	const shouldShowArtistLine = !albumId;
	const shouldShowAlbumCover = !albumId;
	const hasMultipleDiscs = tracks.some((track) => track.discNo > 1);
	const showPlays = (playCounts?.size ?? 0) > 0;
	// The phone keeps the cover or the number, title and artist, duration, menu.
	const numberPhoneClass = shouldShowAlbumCover
		? "phone:hidden"
		: "phone:w-8 phone:pr-0 phone:pl-1";
	const titlePhoneClass = shouldShowAlbumCover
		? "phone:pr-2 phone:pl-0"
		: "phone:px-2";
	const durationPhoneClass = phoneDownloads
		? "phone:hidden"
		: "phone:w-12 phone:px-0";

	return (
		<>
			<table
				ref={tableRef}
				tabIndex={-1}
				className="w-full text-sm outline-none phone:table-fixed"
			>
				<thead className={cn(phoneDownloads && "phone:hidden")}>
					<tr className="border-border border-b text-left text-caption text-xs">
						<th
							className={cn("w-10 font-medium", rowPadding, numberPhoneClass)}
						>
							#
						</th>
						<th className={cn("font-medium", rowPadding, titlePhoneClass)}>
							Title
						</th>
						{showPlays ? (
							<th
								className={cn(
									"w-32 text-right font-medium phone:hidden",
									rowPadding,
								)}
							>
								Plays
							</th>
						) : null}
						<th
							className={cn(
								"w-16 text-right font-medium",
								rowPadding,
								durationPhoneClass,
							)}
						>
							<span className="sr-only">Duration</span>
							<Clock className="ml-auto size-3.5" />
						</th>
						{canFavorite ? (
							<th
								className={cn(
									"w-10 text-center font-medium phone:hidden",
									favoritePadding,
								)}
							>
								<span className="sr-only">Favorite</span>
								<Heart className="mx-auto size-3.5" />
							</th>
						) : null}
						{phoneDownloads ? (
							<th className="hidden w-11 p-0 phone:table-cell">
								<span className="sr-only">Download</span>
							</th>
						) : null}
						<th className="hidden w-11 p-0 phone:table-cell">
							<span className="sr-only">Actions</span>
						</th>
					</tr>
				</thead>
				<tbody>
					{tracks.map((track, index) => {
						const isPlaying = currentTrack?.id === track.id;
						const favorited = isFavorite(track.id);
						const trackNumber = track.trackNo ?? index + 1;
						const visibleNumber =
							numbering === "list"
								? index + 1
								: hasMultipleDiscs
									? `${track.discNo}.${trackNumber}`
									: trackNumber;

						return (
							<ContextMenu key={track.id}>
								<ContextMenuTrigger
									render={
										<tr
											ref={(row) => {
												if (row) rowRefs.current.set(track.id, row);
												else rowRefs.current.delete(track.id);
											}}
											className={cn(
												"group cursor-pointer border-border/70 border-b transition",
												// The playing row takes the active step, above the hover step; flat,
												// no shadow, so it stays part of the list. Its title and equalizer
												// carry the accent.
												isPlaying ? "bg-neutral-active" : "hover:bg-muted",
											)}
											tabIndex={0}
											onClick={() => {
												if (tapPlays) handlePlay(track);
											}}
											onDoubleClick={() => {
												if (!tapPlays) handlePlay(track);
											}}
											onKeyDown={(event) => {
												if (event.key === "Enter") handlePlay(track);
											}}
										/>
									}
								>
									<td
										className={cn(
											"tabular-nums",
											// Caption loses 4.5:1 on the active step.
											isPlaying ? "text-foreground" : "text-caption",
											rowPadding,
											numberPhoneClass,
										)}
									>
										{isPlaying ? (
											<QueueEqualizer
												isPlaying={isPlaybackRunning}
												className="text-neutral-active-foreground"
											/>
										) : (
											visibleNumber
										)}
									</td>
									<td className={cn(rowPadding, titlePhoneClass)}>
										<div className="flex min-w-0 items-center gap-2">
											{shouldShowAlbumCover ? (
												<img
													alt=""
													src={getAlbumCoverUrl(track.albumId, 64)}
													className="size-8 shrink-0 rounded-sm object-cover text-xs"
												/>
											) : null}
											<div className="min-w-0">
												<span className="flex min-w-0 items-center gap-2">
													<span
														className={cn(
															"min-w-0 truncate font-medium text-base leading-snug",
															// The playing title takes the accent, as in the Queue.
															isPlaying
																? "text-neutral-active-foreground"
																: "text-foreground",
														)}
													>
														{track.title}
													</span>
													{track.explicit ? <ExplicitBadge /> : null}
												</span>
												{shouldShowArtistLine ? (
													<span
														className={cn(
															"mt-0.5 block truncate text-xs leading-tight",
															isPlaying ? "text-foreground" : "text-caption",
														)}
													>
														{getTrackArtistName(track)}
													</span>
												) : null}
											</div>
										</div>
									</td>
									{showPlays ? (
										<td
											className={cn(
												"text-right tabular-nums phone:hidden",
												isPlaying ? "text-foreground" : "text-caption",
												rowPadding,
											)}
										>
											{playCounts?.get(track.id)?.toLocaleString() ?? ""}
										</td>
									) : null}
									<td
										className={cn(
											"text-right tabular-nums",
											isPlaying ? "text-foreground" : "text-caption",
											rowPadding,
											durationPhoneClass,
										)}
									>
										{formatDuration(track.durationMs)}
									</td>
									{canFavorite ? (
										<td
											className={cn(
												"text-center phone:hidden",
												favoritePadding,
											)}
										>
											<Button
												type="button"
												variant="ghost"
												size="icon-sm"
												aria-label={
													favorited
														? "Remove from favorites"
														: "Add to favorites"
												}
												className={cn(
													"rounded-full text-caption hover:bg-muted hover:text-heading dark:hover:bg-muted",
													favorited && "text-heading",
												)}
												onClick={(event) => {
													event.stopPropagation();
													toggleFavorite(track.id);
												}}
											>
												<Heart
													className="size-3.5"
													fill={favorited ? "currentColor" : "none"}
												/>
											</Button>
										</td>
									) : null}
									{phoneDownloads ? (
										<td className="hidden w-11 p-0 text-right phone:table-cell">
											<Button
												type="button"
												variant="ghost"
												size="icon-sm"
												aria-label={`Download ${track.title}`}
												title="Download"
												onClick={(event) => {
													event.stopPropagation();
													toast.info("Downloads are not available yet");
												}}
											>
												<Download className="size-4" />
											</Button>
										</td>
									) : null}
									<td className="hidden w-11 p-0 text-right phone:table-cell">
										<RowMenuButton label={`Actions for ${track.title}`} />
									</td>
								</ContextMenuTrigger>
								<ContextMenuContent>
									{canFavorite && isPhone ? (
										<ContextMenuItem onClick={() => toggleFavorite(track.id)}>
											<Heart
												className="size-4"
												fill={favorited ? "currentColor" : "none"}
											/>
											{favorited ? "Remove from favorites" : "Add to favorites"}
										</ContextMenuItem>
									) : null}
									{hideQueueEditing ? null : (
										<>
											<ContextMenuItem
												onClick={() =>
													void handleQueueAction(
														track.id,
														playNext,
														"Failed to play track next",
													)
												}
											>
												<SkipForward className="size-4" />
												Play next
											</ContextMenuItem>
											<ContextMenuItem
												onClick={() =>
													void handleQueueAction(
														track.id,
														addToQueue,
														"Failed to add track to queue",
													)
												}
											>
												<ListPlus className="size-4" />
												Add to queue
											</ContextMenuItem>
										</>
									)}
									<ContextMenuItem
										onClick={() => {
											captureRowFocus(track.id);
											setDetailsTrack(track);
										}}
									>
										<Info className="size-4" />
										Details
									</ContextMenuItem>
									{onRemoveTrack ? (
										<ContextMenuItem onClick={() => onRemoveTrack(track)}>
											<ListMinus className="size-4" />
											{removeLabel}
										</ContextMenuItem>
									) : null}
									{showDelete &&
									hasReplacementCapability &&
									!hideReplacement ? (
										<ContextMenuItem
											disabled={trackReplacement.isBusy}
											onClick={() => {
												captureRowFocus(track.id);
												trackReplacement.open(track);
											}}
										>
											<Replace className="size-4" />
											Replace file
										</ContextMenuItem>
									) : null}
									{showDelete && hasDeletionCapability && !hideDeletion ? (
										<ContextMenuItem
											variant="destructive"
											disabled={trackDeletion.isDeleting}
											onClick={() => {
												captureRowFocus(track.id);
												trackDeletion.open(track);
											}}
										>
											<Trash2 className="size-4" />
											Delete track
										</ContextMenuItem>
									) : null}
								</ContextMenuContent>
							</ContextMenu>
						);
					})}
				</tbody>
			</table>
			<TrackDetailsDialog
				track={detailsTrack}
				onOpenChange={(isOpen) => {
					if (!isOpen) setDetailsTrack(null);
				}}
				finalFocus={restoreRowFocus}
			/>
			<TrackDeletionDialog
				track={trackDeletion.track}
				finalFocus={restoreRowFocus}
				preview={trackDeletion.preview}
				error={trackDeletion.error}
				isLoading={trackDeletion.isLoading}
				isDeleting={trackDeletion.isDeleting}
				onCancel={trackDeletion.cancel}
				onConfirm={trackDeletion.confirm}
			/>
			<TrackReplacementDialog
				track={trackReplacement.track}
				finalFocus={restoreRowFocus}
				step={trackReplacement.step}
				preview={trackReplacement.preview}
				progress={trackReplacement.progress}
				error={trackReplacement.error}
				isBusy={trackReplacement.isBusy}
				isDesktop={trackReplacement.isDesktop}
				onCancel={() => void trackReplacement.cancel()}
				onClose={trackReplacement.close}
				onFile={(file) => void trackReplacement.replaceWith(file)}
				onSelectDesktopFile={() => void trackReplacement.selectDesktopFile()}
				onConfirm={() => void trackReplacement.confirm()}
			/>
		</>
	);
}

function TrackDetailsDialog({
	track,
	onOpenChange,
	finalFocus,
}: {
	track: Track | null;
	onOpenChange: (isOpen: boolean) => void;
	finalFocus?: () => HTMLElement | true;
}) {
	const rows = track ? buildTrackDetailRows(track) : [];

	return (
		<Dialog open={Boolean(track)} onOpenChange={onOpenChange}>
			<DialogContent
				finalFocus={finalFocus}
				className="max-h-dialog-height max-w-2xl overflow-hidden p-0"
			>
				<div className="flex items-center justify-between gap-3 border-border border-b p-4">
					<DialogTitle className="truncate">{track?.title}</DialogTitle>
					<DialogClose
						render={<Button type="button" variant="ghost" size="icon-sm" />}
					>
						<span className="sr-only">Close</span>
						<X className="size-4" />
					</DialogClose>
				</div>
				{rows.length > 0 ? (
					<div className="max-h-dialog-body-height overflow-auto p-4">
						{rows.map(([label, value]) => (
							<DetailRow key={label} label={label} value={value} />
						))}
					</div>
				) : null}
			</DialogContent>
		</Dialog>
	);
}

function DetailRow({ label, value }: { label: string; value?: string | null }) {
	if (!value) return null;
	return (
		<div className="grid grid-cols-label gap-3 border-border border-b py-2 text-sm">
			<span className="text-caption">{label}</span>
			<span className="min-w-0 break-words font-medium text-foreground">
				{value}
			</span>
		</div>
	);
}
