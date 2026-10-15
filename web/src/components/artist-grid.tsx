import type { Artist } from "@repo/api-client";
import { CollectionGrid } from "#/components/collection-grid-layout";

export function ArtistGrid({ artists }: { artists: Artist[] }) {
	return (
		<CollectionGrid>
			{artists.map((artist) => (
				<div
					key={artist.id}
					className="group flex aspect-square min-w-0 transform-gpu flex-col overflow-hidden rounded-lg border border-border p-3 backface-hidden transition duration-300 ease-out hover:-translate-y-1 hover:border-[var(--shell-active-foreground)]/35 hover:bg-muted/50 hover:shadow-[0_18px_40px_-18px_rgb(0_0_0/0.75)]"
				>
					<div className="mb-3 flex min-h-0 flex-1 transform-gpu items-center justify-center rounded-md bg-muted font-semibold text-2xl uppercase transition duration-500 ease-out group-hover:scale-[1.04]">
						{artist.name.slice(0, 1)}
					</div>
					<p className="truncate font-medium text-heading text-sm">
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
