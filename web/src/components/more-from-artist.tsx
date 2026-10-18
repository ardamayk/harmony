import { useQuery } from "@tanstack/react-query";
import { AlbumShelf } from "#/components/album-shelf";
import { apiClient } from "#/lib/api";
import { sortAlbumsByYear } from "#/lib/sort-albums";

export function MoreFromArtist({
	artistId,
	artistName,
	excludeAlbumId,
}: {
	artistId: string;
	artistName: string;
	excludeAlbumId: string;
}) {
	const albums = useQuery({
		queryKey: ["library", "albums", "artist", artistId],
		queryFn: () => apiClient.listAlbums({ artistId, limit: 100 }),
		staleTime: 60_000,
	});

	const items = sortAlbumsByYear(
		(albums.data?.items ?? []).filter((album) => album.id !== excludeAlbumId),
	);

	if (albums.isLoading) {
		return (
			<section className="mt-6">
				<h2 className="mb-4 font-semibold text-heading text-xl">
					More from {artistName}
				</h2>
				<p className="text-foreground text-sm">Loading albums…</p>
			</section>
		);
	}

	if (items.length === 0) {
		return null;
	}

	return (
		<section className="mt-6">
			<AlbumShelf
				heading={
					<h2 className="font-semibold text-heading text-xl">
						More from {artistName}
					</h2>
				}
				albums={items}
			/>
		</section>
	);
}
