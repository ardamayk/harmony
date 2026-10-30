import type { Track } from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { useHiddenFromGuest } from "@repo/ui/guest-access";
import { SearchField } from "#/components/search-field";
import {
	ListMusic,
	Music2,
	Play,
	Shuffle,
	SkipForward,
	Tags,
} from "lucide-react";
import { CollectionCoverStack } from "#/components/collection-cover-strip";
import {
	DetailStickyAction,
	DetailStickyBar,
	useScrolledPast,
} from "#/components/detail-sticky-bar";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";

export function CollectionDetailHeader({
	kind,
	title,
	subtitle,
	metaTags,
	trackCount,
	coverTracks,
	searchValue,
	searchPlaceholder,
	onSearchChange,
	onPlay,
	onShuffle,
	onQueue,
}: {
	kind: "Playlist" | "Genre";
	title: string;
	subtitle: string;
	metaTags: string[];
	trackCount: number;
	coverTracks?: Track[];
	searchValue?: string;
	searchPlaceholder?: string;
	onSearchChange?: (value: string) => void;
	onPlay: () => void;
	onShuffle: () => void;
	onQueue: () => void;
}) {
	const disabled = trackCount === 0;
	const Icon = kind === "Genre" ? Tags : ListMusic;
	const showSearch = searchValue !== undefined && onSearchChange;
	const hideQueueEditing = useHiddenFromGuest("queue-editing");
	const [setActionRow, actionsScrolledPast] = useScrolledPast();
	const firstCoverTrack = coverTracks?.[0];

	return (
		<>
			<DetailStickyBar
				visible={actionsScrolledPast}
				cover={
					firstCoverTrack ? (
						<AlbumArt
							coverUrl={apiClient.getAlbumCoverUrl(
								firstCoverTrack.albumId,
								128,
							)}
							title={title}
							className="size-16 shrink-0 rounded-sm text-xs"
						/>
					) : (
						<div className="flex size-16 shrink-0 items-center justify-center rounded-sm bg-muted text-caption">
							<Icon className="size-6" />
						</div>
					)
				}
				title={title}
				subtitle={subtitle}
			>
				<DetailStickyAction
					icon={Play}
					label="Play"
					filled
					disabled={disabled}
					onClick={onPlay}
				/>
				<DetailStickyAction
					icon={Shuffle}
					label="Shuffle"
					disabled={disabled}
					onClick={onShuffle}
				/>
				{hideQueueEditing ? null : (
					<DetailStickyAction
						icon={SkipForward}
						label="Queue"
						disabled={disabled}
						onClick={onQueue}
					/>
				)}
			</DetailStickyBar>
			<div>
				{/* `isolate` keeps the stacked covers' z-index under the sticky bar. */}
				<div className="isolate grid gap-6 sm:grid-cols-media sm:items-stretch">
					{coverTracks && coverTracks.length > 0 ? (
						<CollectionCoverStack tracks={coverTracks} />
					) : (
						<div className="flex size-32 shrink-0 items-center justify-center rounded-md bg-muted text-caption shadow-lg">
							<Icon className="size-12" />
						</div>
					)}

					<div className="min-w-0">
						<p className="mb-2 font-semibold text-caption text-xs uppercase tracking-widest">
							{kind}
						</p>
						<h1 className="truncate font-semibold text-page-title tracking-tight">
							{title}
						</h1>
						<p className="mt-2 font-medium text-foreground text-xl">
							{subtitle}
						</p>

						<div className="mt-4 flex flex-wrap items-center gap-2 text-foreground text-sm">
							<Music2 className="size-4 shrink-0" />
							{metaTags.map((tag) => (
								<Badge key={tag} variant="secondary">
									{tag}
								</Badge>
							))}
						</div>

						<div
							ref={setActionRow}
							className="mt-6 flex flex-wrap items-center gap-2"
						>
							<div className="flex flex-wrap gap-2">
								<Button type="button" disabled={disabled} onClick={onPlay}>
									<Play className="size-4" />
									Play
								</Button>
								<Button
									type="button"
									variant="outline"
									disabled={disabled}
									onClick={onShuffle}
								>
									<Shuffle className="size-4" />
									Shuffle
								</Button>
								{hideQueueEditing ? null : (
									<Button
										type="button"
										variant="outline"
										disabled={disabled}
										onClick={onQueue}
									>
										<SkipForward className="size-4" />
										Queue
									</Button>
								)}
							</div>
							{showSearch ? (
								<SearchField
									className="ml-auto w-full sm:max-w-xs"
									placeholder={
										searchPlaceholder ?? `Search ${kind.toLowerCase()}…`
									}
									value={searchValue}
									onChange={(event) => onSearchChange(event.target.value)}
								/>
							) : null}
						</div>
					</div>
				</div>
			</div>
		</>
	);
}
