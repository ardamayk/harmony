import {
	type Album,
	MANAGED_ALBUM_DELETION_CAPABILITY,
} from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Link } from "@tanstack/react-router";
import { Trash2 } from "lucide-react";
import { AlbumDeletionDialog } from "#/components/album-deletion-dialog";
import {
	CollectionGrid,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
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
import { cn } from "#/lib/utils";

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
						<ContextMenuTrigger
							render={
								<Link
									to="/library/$albumId"
									params={{ albumId: album.id }}
									className={cn(
										"group relative aspect-square overflow-hidden rounded-md border border-border bg-card",
										CARD_INTERACTION_CLASS,
									)}
								/>
							}
						>
							<AlbumArt
								coverUrl={apiClient.getAlbumCoverUrl(album.id, 512)}
								title={album.title}
								className="absolute inset-0 size-full transform-gpu text-2xl transition duration-500 ease-out group-hover:scale-104 group-focus-visible:scale-104"
							/>
							<div
								data-album-card-overlay
								className="absolute inset-x-0 bottom-0 z-10 bg-gradient-to-t from-background/95 via-background/75 to-transparent p-2 text-foreground"
							>
								<p
									className="truncate font-medium text-heading text-xs leading-tight"
									title={album.title}
								>
									{album.title}
								</p>
								<p className="truncate text-xs text-foreground leading-tight">
									{getAlbumArtistName(album)}
								</p>
								{album.releaseDate != null || album.year != null ? (
									<p className="text-xs text-caption leading-tight">
										{album.releaseDate ?? album.year}
									</p>
								) : null}
							</div>
						</ContextMenuTrigger>
						<ContextMenuContent>
							<ContextMenuItem
								render={
									<Link to="/library/$albumId" params={{ albumId: album.id }} />
								}
							>
								Open album
							</ContextMenuItem>
							{canDeleteAlbums ? (
								<>
									<ContextMenuSeparator />
									<ContextMenuItem
										variant="destructive"
										onClick={() => albumDeletion.open(album)}
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
