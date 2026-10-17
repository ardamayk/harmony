import type { Artist } from "@repo/api-client";
import {
	CollectionGrid,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
import { cn } from "#/lib/utils";

export function ArtistGrid({ artists }: { artists: Artist[] }) {
	return (
		<CollectionGrid>
			{artists.map((artist) => (
				<div
					key={artist.id}
					className={cn(
						"group flex aspect-square min-w-0 flex-col overflow-hidden rounded-md border border-border p-3 hover:bg-muted/50",
						CARD_INTERACTION_CLASS,
					)}
				>
					<div className="mb-3 flex min-h-0 flex-1 transform-gpu items-center justify-center rounded-sm bg-muted font-semibold text-2xl uppercase transition duration-500 ease-out group-hover:scale-104">
						{artist.name.slice(0, 1)}
					</div>
					<p
						className="truncate font-medium text-heading text-sm"
						title={artist.name}
					>
						{artist.name}
					</p>
					{artist.albumCount != null ? (
						<p className="text-foreground text-xs">
							{artist.albumCount} albums
						</p>
					) : null}
				</div>
			))}
		</CollectionGrid>
	);
}
