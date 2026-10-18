import type { Artist } from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Link } from "@tanstack/react-router";
import {
	CollectionGrid,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
import { apiClient } from "#/lib/api";
import { cn } from "#/lib/utils";

/** Artist cards share the Album card's look: photo (or initial) and caption. */
export function ArtistGrid({ artists }: { artists: Artist[] }) {
	return (
		<CollectionGrid>
			{artists.map((artist) => (
				<Link
					key={artist.id}
					to="/library/artists/$artistId"
					params={{ artistId: artist.id }}
					className={cn(
						"group relative aspect-square overflow-hidden rounded-md border border-border bg-card",
						CARD_INTERACTION_CLASS,
					)}
				>
					<AlbumArt
						coverUrl={apiClient.getArtistPhotoUrl(artist.id)}
						title={artist.name}
						className="absolute inset-0 size-full transform-gpu text-2xl transition duration-500 ease-out group-hover:scale-104 group-focus-visible:scale-104"
					/>
					<div className="absolute inset-x-0 bottom-0 z-10 bg-gradient-to-t from-background/97 via-background/86 via-45% to-transparent px-2 pt-12 pb-2 text-foreground">
						<p
							className="truncate font-semibold text-heading text-xs leading-tight"
							title={artist.name}
						>
							{artist.name}
						</p>
						{artist.albumCount != null ? (
							<p className="text-xs text-caption leading-tight">
								{artist.albumCount === 1
									? "1 album"
									: `${artist.albumCount} albums`}
							</p>
						) : null}
					</div>
				</Link>
			))}
		</CollectionGrid>
	);
}
