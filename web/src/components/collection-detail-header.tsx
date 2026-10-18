import type { Track } from "@repo/api-client";
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
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";

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

	return (
		<div>
			<div className="grid gap-6 sm:grid-cols-media sm:items-stretch">
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
					<h1 className="truncate font-semibold text-3xl tracking-tight">
						{title}
					</h1>
					<p className="mt-2 font-medium text-foreground text-xl">{subtitle}</p>

					<div className="mt-4 flex flex-wrap items-center gap-2 text-foreground text-sm">
						<Music2 className="size-4 shrink-0" />
						{metaTags.map((tag) => (
							<Badge key={tag} variant="secondary">
								{tag}
							</Badge>
						))}
					</div>

					<div className="mt-6 flex flex-wrap items-center gap-2">
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
							<Button
								type="button"
								variant="outline"
								disabled={disabled}
								onClick={onQueue}
							>
								<SkipForward className="size-4" />
								Queue
							</Button>
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
	);
}
