import {
	type AlbumDetail,
	MANAGED_ALBUM_DELETION_CAPABILITY,
} from "@repo/api-client";
import {
	AlbumArt,
	useCoverAccent,
	useLayout,
	useResolvedThemeMode,
} from "@repo/ui";
import type { CSSProperties } from "react";
import { Disc3, Play, SkipForward, Trash2 } from "lucide-react";
import { AlbumDeletionDialog } from "#/components/album-deletion-dialog";
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
	const { preferences } = useLayout();
	// Same cache key and size as the Player Bar, so the cover is read once.
	const coverAccent = useCoverAccent({
		cacheKey: album.id,
		coverUrl: apiClient.getAlbumCoverUrl(album.id, 128),
		enabled: true,
		mode: useResolvedThemeMode(preferences.theme.mode),
	});
	return (
		// Bleeds to the scroller's edges, behind the Queue Drawer too; the wash
		// ends where the track list begins. No cover colour yet leaves the plain
		// page background.
		<div
			className="-mt-6 -ml-(--shell-inset) -mr-(--page-edge-right) bg-linear-to-b from-(--cover-tint)/20 to-(--cover-tint)/5 pt-6 pr-(--page-edge-right) pb-6 pl-(--shell-inset)"
			style={{ "--cover-tint": coverAccent["--cover-tint"] } as CSSProperties}
		>
			<ContextMenu>
				<ContextMenuTrigger render={<div className="@container" />}>
					<div className="grid gap-8 @xl:grid-cols-media @xl:items-end">
						<AlbumArt
							coverUrl={apiClient.getAlbumCoverUrl(album.id, 1024)}
							title={album.title}
							className="aspect-square w-full shrink-0 rounded-md shadow-lg text-3xl @xl:size-52"
						/>

						<div className="flex min-w-0 flex-col">
							<h1 className="font-semibold text-3xl tracking-tight">
								{album.title}
							</h1>
							<p className="mt-2 font-medium text-foreground text-xl">
								{artistName}
							</p>

							{/* Release date under the artist, track count and length last;
							    both on the caption step. */}
							<div className="mt-4 flex flex-col gap-1 text-caption text-sm">
								{released ? <p>{released}</p> : null}
								{size ? <p>{size}</p> : null}
							</div>
						</div>
					</div>

					{/* Actions under the cover, external links at the far right. */}
					<div className="mt-6 flex flex-wrap items-center gap-2">
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
		</div>
	);
}
