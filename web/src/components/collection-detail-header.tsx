import type { Track } from "@repo/api-client";
import {
	ListMusic,
	Music2,
	Play,
	Search,
	Shuffle,
	SkipForward,
	Tags,
} from "lucide-react";
import { CollectionCoverStack } from "#/components/collection-cover-strip";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";

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
		<div className="relative overflow-hidden rounded-lg border border-border/60 bg-card/40">
			<div className="pointer-events-none absolute inset-0 bg-gradient-to-br from-primary/20 via-background/40 to-background" />
			<div className="relative grid gap-6 p-6 sm:grid-cols-[auto_minmax(0,1fr)] sm:items-stretch">
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
							<Badge key={tag} variant="secondary" className="font-normal">
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
							<div className="relative ml-auto w-full sm:max-w-xs">
								<Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-caption" />
								<Input
									className="pl-8"
									placeholder={
										searchPlaceholder ?? `Search ${kind.toLowerCase()}…`
									}
									value={searchValue}
									onChange={(event) => onSearchChange(event.target.value)}
								/>
							</div>
						) : null}
					</div>
				</div>
			</div>
		</div>
	);
}
