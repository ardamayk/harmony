import { AlbumArt } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { AlbumGrid } from "#/components/album-grid";
import { CoverWash } from "#/components/cover-wash";
import { openExternalLink } from "#/components/external-link-button";
import { DetailPageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";
import { sortAlbumsByYear } from "#/lib/sort-albums";

// Route components live outside the route file (see -album-page.tsx).
export function ArtistDetailPage() {
	const { artistId } = useParams({ from: "/library/artists/$artistId" });
	return <ArtistDetailContent artistId={artistId} />;
}

export function ArtistDetailContent({ artistId }: { artistId: string }) {
	const artist = useQuery({
		queryKey: ["library", "artist", artistId],
		queryFn: () => apiClient.getArtist(artistId),
	});
	const albums = useQuery({
		queryKey: ["library", "albums", "artist", artistId],
		queryFn: () => apiClient.listAlbums({ artistId, limit: 100 }),
		staleTime: 60_000,
	});
	// The Music Server looks it up online once and stores it; asked again on
	// each visit, since a lookup that failed earlier may have found it since.
	const biography = useQuery({
		queryKey: ["library", "artist", artistId, "biography"],
		queryFn: () => apiClient.getArtistBiography(artistId),
	});

	if (artist.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading artist…</div>;
	}
	if (artist.isError || !artist.data) {
		return <div className="p-6 text-destructive text-sm">Artist not found</div>;
	}

	const { name, albumCount = 0 } = artist.data;
	const photoUrl = apiClient.getArtistPhotoUrl(artistId);
	const albumItems = sortAlbumsByYear(albums.data?.items ?? []);

	return (
		<DetailPageShell testId="artist-detail-content">
			<CoverWash cacheKey={`artist:${artistId}`} imageUrl={photoUrl}>
				<div className="@container">
					<div className="grid gap-8 @xl:grid-cols-media @xl:items-end">
						<AlbumArt
							coverUrl={photoUrl}
							title={name}
							className="aspect-square w-full shrink-0 rounded-full shadow-lg text-3xl @xl:size-52"
						/>
						<div className="flex min-w-0 flex-col">
							<h1 className="font-semibold text-3xl tracking-tight">{name}</h1>
							<p className="mt-4 text-caption text-sm">
								{albumCount === 1 ? "1 album" : `${albumCount} albums`}
							</p>
						</div>
					</div>
				</div>
			</CoverWash>

			<section className="mt-6">
				<h2 className="mb-4 font-semibold text-heading text-xl">Albums</h2>
				{albums.isLoading ? (
					<p className="text-foreground text-sm">Loading albums…</p>
				) : albumItems.length > 0 ? (
					<AlbumGrid albums={albumItems} />
				) : (
					<p className="text-caption text-sm">
						No albums under this artist's name.
					</p>
				)}
			</section>

			{biography.data ? (
				<section className="mt-6 max-w-prose">
					<h2 className="mb-4 font-semibold text-heading text-xl">About</h2>
					<p className="text-foreground text-sm leading-relaxed">
						{biography.data.text}
					</p>
					{/* Wikipedia's CC BY-SA licence asks for the source with the text. */}
					<a
						href={biography.data.sourceUrl}
						target="_blank"
						rel="noopener noreferrer"
						onClick={openExternalLink(biography.data.sourceUrl)}
						className="mt-3 inline-block text-caption text-xs hover:text-heading"
					>
						From Wikipedia, CC BY-SA
					</a>
				</section>
			) : null}
		</DetailPageShell>
	);
}
