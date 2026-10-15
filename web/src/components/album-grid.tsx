import {
	type Album,
	MANAGED_ALBUM_DELETION_CAPABILITY,
} from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Link } from "@tanstack/react-router";
import { Trash2 } from "lucide-react";
import { AlbumDeletionDialog } from "#/components/album-deletion-dialog";
import { CollectionGrid } from "#/components/collection-grid-layout";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import { useAlbumDeletionFlow } from "#/hooks/use-album-deletion-flow";
import { useServerCapability } from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";
import { getAlbumArtistName } from "#/lib/library-display";

export function AlbumGrid({ albums }: { albums: Album[] }) {
	const albumDeletion = useAlbumDeletionFlow();
	const canDeleteAlbums = useServerCapability(
		MANAGED_ALBUM_DELETION_CAPABILITY,
	);
	return (
		<>
			<CollectionGrid>
				{albums.map((album) => (
					<ContextMenu key={album.id}>
						<ContextMenuTrigger asChild>
							<Link
								to="/library/$albumId"
								params={{ albumId: album.id }}
								className="group relative aspect-square transform-gpu overflow-hidden rounded-md border border-border bg-card outline-none backface-hidden transition duration-300 ease-out hover:-translate-y-1 hover:border-[var(--shell-active-foreground)]/35 hover:shadow-[0_18px_40px_-18px_rgb(0_0_0/0.75)] focus-visible:-translate-y-1 focus-visible:border-[var(--shell-active-foreground)]/60 focus-visible:shadow-[0_18px_40px_-18px_rgb(0_0_0/0.75)]"
							>
								<AlbumArt
									coverUrl={apiClient.getAlbumCoverUrl(album.id)}
									title={album.title}
									className="absolute inset-0 size-full transform-gpu text-2xl transition duration-500 ease-out group-hover:scale-[1.04] group-focus-visible:scale-[1.04]"
								/>
								<div
									data-album-card-overlay
									className="absolute inset-x-0 bottom-0 z-10 bg-gradient-to-t from-background/95 via-background/75 to-transparent p-2 text-foreground"
								>
									<p className="truncate font-medium text-heading text-xs leading-tight">
										{album.title}
									</p>
									<p className="truncate text-[11px] text-foreground leading-tight">
										{getAlbumArtistName(album)}
									</p>
									{album.releaseDate != null || album.year != null ? (
										<p className="text-[10px] text-caption leading-tight">
											{album.releaseDate ?? album.year}
										</p>
									) : null}
								</div>
							</Link>
						</ContextMenuTrigger>
						<ContextMenuContent>
							<ContextMenuItem asChild>
								<Link to="/library/$albumId" params={{ albumId: album.id }}>
									Open album
								</Link>
							</ContextMenuItem>
							{canDeleteAlbums ? (
								<>
									<ContextMenuSeparator />
									<ContextMenuItem
										variant="destructive"
										onSelect={() => albumDeletion.open(album)}
									>
										<Trash2 className="size-4" />
										Delete album
									</ContextMenuItem>
								</>
							) : null}
						</ContextMenuContent>
					</ContextMenu>
				))}
			</CollectionGrid>
			<AlbumDeletionDialog
				album={albumDeletion.album}
				preview={albumDeletion.preview}
				error={albumDeletion.error}
				isLoading={albumDeletion.isLoading}
				isDeleting={albumDeletion.isDeleting}
				onCancel={albumDeletion.cancel}
				onConfirm={albumDeletion.confirm}
			/>
		</>
	);
}
