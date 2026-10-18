import {
	type AlbumDetail,
	MANAGED_ALBUM_DELETION_CAPABILITY,
} from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Link } from "@tanstack/react-router";
import { Disc3, Play, SkipForward, Trash2 } from "lucide-react";
import { AlbumDeletionDialog } from "#/components/album-deletion-dialog";
import { CoverWash } from "#/components/cover-wash";
import { ExternalLinkButton } from "#/components/external-link-button";
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
	const trackCount = album.trackCount ?? album.tracks.length;

	const size = [
		trackCount > 0 ? `${trackCount} tracks` : null,
		totalDurationMs > 0 ? formatTotalDuration(totalDurationMs) : null,
	]
		.filter(Boolean)
		.join(" · ");
	const released =
		album.releaseDate ?? (album.year != null ? String(album.year) : null);

	const artistName = getAlbumArtistName(album);
	const externalLinks = getAlbumExternalLinks(artistName, album.title);
	const albumDeletion = useAlbumDeletionFlow();
	const canDeleteAlbums = useServerCapability(
		MANAGED_ALBUM_DELETION_CAPABILITY,
	);
	return (
		// Same cache key and size as the Player Bar, so the cover is read once.
		<CoverWash
			cacheKey={album.id}
			imageUrl={apiClient.getAlbumCoverUrl(album.id, 128)}
		>
			<ContextMenu>
				<ContextMenuTrigger render={<div className="@container" />}>
					<div className="grid gap-8 @xl:grid-cols-media @xl:items-end">
						<AlbumArt
							coverUrl={apiClient.getAlbumCoverUrl(album.id, 1024)}
							title={album.title}
							className="aspect-square w-full shrink-0 rounded-md shadow-lg text-3xl @xl:size-60"
						/>

						<div className="flex min-w-0 flex-col">
							<h1 className="font-semibold text-page-title tracking-tight">
								{album.title}
							</h1>
							<Link
								to="/library/artists/$artistId"
								params={{ artistId: album.artistId }}
								className="mt-3 self-start font-medium text-base text-foreground hover:text-heading"
							>
								{artistName}
							</Link>

							{/* Release date under the artist, track count and length last;
							    both on the caption step. */}
							<div className="mt-6 flex flex-col gap-2 text-caption text-sm">
								{released ? <p>{released}</p> : null}
								{size ? <p>{size}</p> : null}
							</div>
						</div>
					</div>

					{/* Actions under the cover, external links at the far right. */}
					<div className="mt-6 flex flex-wrap items-center gap-2">
						<Button type="button" size="lg" onClick={onPlayAlbum}>
							<Play className="size-4" />
							Play
						</Button>
						<Button
							type="button"
							size="lg"
							variant="outline"
							onClick={onQueueAlbum}
						>
							<SkipForward className="size-4" />
							Queue album
						</Button>
						<Button
							type="button"
							size="lg"
							variant="outline"
							disabled
							title="Album radio will build a station from similar album, artist, and genre tracks when recommendations exist."
						>
							<Disc3 className="size-4" />
							Album radio
						</Button>
						<div className="ml-auto flex gap-2">
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
				</ContextMenuTrigger>
				<ContextMenuContent>
					{canDeleteAlbums ? (
						<ContextMenuItem
							variant="destructive"
							onClick={() => albumDeletion.open(album)}
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
		</CoverWash>
	);
}
