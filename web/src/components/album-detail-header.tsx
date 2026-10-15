import {
	type AlbumDetail,
	MANAGED_ALBUM_DELETION_CAPABILITY,
} from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Clock, Disc3, Music2, Play, SkipForward, Trash2 } from "lucide-react";
import { AlbumDeletionDialog } from "#/components/album-deletion-dialog";
import { ExternalLinkButton } from "#/components/external-link-button";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import { useAlbumDeletionFlow } from "#/hooks/use-album-deletion-flow";
import { useServerCapability } from "#/hooks/use-server-capability";
import { getAlbumExternalLinks } from "#/lib/album-external-links";
import { getAlbumGenres } from "#/lib/album-genres";
import { apiClient } from "#/lib/api";
import { getAlbumArtistName } from "#/lib/library-display";

function formatTotalDuration(ms: number): string {
	if (!ms || ms < 0) return "0m";
	const total = Math.floor(ms / 1000);
	const hours = Math.floor(total / 3600);
	const minutes = Math.floor((total % 3600) / 60);
	if (hours > 0) {
		return `${hours}h ${minutes}m`;
	}
	return `${minutes}m`;
}

function uniqueFormats(tracks: AlbumDetail["tracks"]): string[] {
	return [...new Set(tracks.map((t) => t.format.toUpperCase()))];
}

export function AlbumDetailHeader({
	album,
	onPlayAlbum,
	onQueueAlbum,
}: {
	album: AlbumDetail;
	onPlayAlbum: () => void;
	onQueueAlbum: () => void;
}) {
	const totalDurationMs = album.tracks.reduce(
		(sum, track) => sum + (track.durationMs ?? 0),
		0,
	);
	const formats = uniqueFormats(album.tracks);
	const trackCount = album.trackCount ?? album.tracks.length;

	const metaTags = [
		album.releaseDate ?? (album.year != null ? String(album.year) : null),
		trackCount > 0 ? `${trackCount} tracks` : null,
		totalDurationMs > 0 ? formatTotalDuration(totalDurationMs) : null,
		...formats,
	].filter(Boolean) as string[];

	const artistName = getAlbumArtistName(album);
	const externalLinks = getAlbumExternalLinks(artistName, album.title);
	const genres = getAlbumGenres(album);
	const albumDeletion = useAlbumDeletionFlow();
	const canDeleteAlbums = useServerCapability(
		MANAGED_ALBUM_DELETION_CAPABILITY,
	);
	return (
		<ContextMenu>
			<ContextMenuTrigger asChild>
				<div className="@container relative overflow-hidden rounded-xl border border-border/60 bg-card/40">
					<div className="pointer-events-none absolute inset-0 bg-gradient-to-br from-primary/20 via-background/40 to-background" />
					<div className="relative grid gap-8 p-6 @xl:grid-cols-[auto_minmax(0,1fr)] @xl:items-start @4xl:grid-cols-[auto_minmax(0,1fr)_220px]">
						<AlbumArt
							coverUrl={apiClient.getAlbumCoverUrl(album.id)}
							title={album.title}
							className="aspect-square w-full shrink-0 rounded-lg shadow-lg text-5xl @xl:size-52"
						/>

						<div className="min-w-0 pt-1">
							<p className="mb-2 font-semibold text-caption text-xs uppercase tracking-widest">
								Album
							</p>
							<h1 className="font-semibold text-3xl tracking-tight sm:text-4xl">
								{album.title}
							</h1>
							<p className="mt-2 font-medium text-foreground text-lg">
								{artistName}
							</p>

							<div className="mt-4 flex flex-wrap items-center gap-2 text-foreground text-sm">
								<Music2 className="size-4 shrink-0" />
								{metaTags.map((tag) => (
									<Badge key={tag} variant="secondary" className="font-normal">
										{tag}
									</Badge>
								))}
							</div>

							<div className="mt-6 flex flex-wrap gap-2">
								<Button type="button" onClick={onPlayAlbum}>
									<Play className="size-4" />
									Play
								</Button>
								<Button type="button" variant="outline" onClick={onQueueAlbum}>
									<SkipForward className="size-4" />
									Queue album
								</Button>
								<Button
									type="button"
									variant="outline"
									disabled
									title="Album radio will build a station from similar album, artist, and genre tracks when recommendations exist."
								>
									<Disc3 className="size-4" />
									Album radio
								</Button>
							</div>
						</div>

						<div className="flex flex-wrap gap-x-12 gap-y-6 @xl:col-span-2 @4xl:col-span-1 @4xl:flex-col @4xl:flex-nowrap @4xl:pt-2">
							<div>
								<p className="mb-2 font-semibold text-caption text-xs uppercase tracking-widest">
									Genres
								</p>
								{genres.length > 0 ? (
									<div className="flex flex-wrap gap-2">
										{genres.map((genre) => (
											<Badge key={genre} variant="outline" className="text-sm">
												{genre}
											</Badge>
										))}
									</div>
								) : (
									<p className="text-foreground text-sm">Not tagged</p>
								)}
							</div>

							<div>
								<p className="mb-3 font-semibold text-caption text-xs uppercase tracking-widest">
									External links
								</p>
								<div className="flex flex-wrap gap-2">
									{externalLinks.map((link) => (
										<ExternalLinkButton
											key={link.id}
											href={link.href}
											name={link.name}
											short={link.short}
											iconSrc={link.iconSrc}
											iconClassName={link.iconClassName}
										/>
									))}
								</div>
							</div>

							<div className="flex items-center gap-2 self-end text-caption text-xs @4xl:self-auto">
								<Clock className="size-3.5" />
								<span>
									{totalDurationMs > 0
										? `${formatTotalDuration(totalDurationMs)} total`
										: "Duration unknown"}
								</span>
							</div>
						</div>
					</div>
				</div>
			</ContextMenuTrigger>
			<ContextMenuContent>
				{canDeleteAlbums ? (
					<ContextMenuItem
						variant="destructive"
						onSelect={() => albumDeletion.open(album)}
					>
						<Trash2 className="size-4" />
						Delete album
					</ContextMenuItem>
				) : null}
			</ContextMenuContent>
			<AlbumDeletionDialog
				album={albumDeletion.album}
				preview={albumDeletion.preview}
				error={albumDeletion.error}
				isLoading={albumDeletion.isLoading}
				isDeleting={albumDeletion.isDeleting}
				onCancel={albumDeletion.cancel}
				onConfirm={albumDeletion.confirm}
			/>
		</ContextMenu>
	);
}
