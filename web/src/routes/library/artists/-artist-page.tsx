import type { Album } from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import { AlbumGrid } from "#/components/album-grid";
import { CoverWash } from "#/components/cover-wash";
import { openExternalLink } from "#/components/external-link-button";
import { DetailPageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";
import { sortAlbumsByYear } from "#/lib/sort-albums";
import { isGone } from "#/lib/query-persistence";

// Albums carry no release type, so the track count decides: a single or an
// EP has at most six tracks, as the stores count them.
const MAX_SINGLE_OR_EP_TRACKS = 6;
// Popular Tracks shown at first, and after See more.
const POPULAR_SHOWN = 5;
const POPULAR_MORE = 10;
// A summary this long is clipped to three lines until Read more.
const LONG_BIOGRAPHY_CHARS = 240;

type DiscographyFilter = "all" | "albums" | "singles";

function isSingleOrEp(album: Album) {
	return (
		album.trackCount != null &&
		album.trackCount > 0 &&
		album.trackCount <= MAX_SINGLE_OR_EP_TRACKS
	);
}

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
	// Deezer's most popular, limited to the library's Tracks; stored by the
	// Music Server, which refreshes it weekly.
	const popular = useQuery({
		queryKey: ["library", "artist", artistId, "popular-tracks"],
		queryFn: () => apiClient.listArtistPopularTracks(artistId, POPULAR_MORE),
		staleTime: 60_000,
	});
	const [popularExpanded, setPopularExpanded] = useState(false);
	const [bioExpanded, setBioExpanded] = useState(false);
	const [filter, setFilter] = useState<DiscographyFilter>("all");

	if (artist.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading artist…</div>;
	}
	if (!artist.data || isGone(artist.error)) {
		return <div className="p-6 text-destructive text-sm">Artist not found</div>;
	}

	const { name, albumCount = 0 } = artist.data;
	const photoUrl = apiClient.getArtistPhotoUrl(artistId);
	const albumItems = sortAlbumsByYear(albums.data?.items ?? []);
	const singles = albumItems.filter(isSingleOrEp);
	const fullAlbums = albumItems.filter((album) => !isSingleOrEp(album));
	// Chips only when both kinds exist; one kind alone needs no filter.
	const filters: Array<{ id: DiscographyFilter; label: string }> =
		singles.length > 0 && fullAlbums.length > 0
			? [
					{ id: "all", label: "All" },
					{ id: "albums", label: "Albums" },
					{ id: "singles", label: "Singles and EPs" },
				]
			: [];
	const shownAlbums =
		filter === "albums"
			? fullAlbums
			: filter === "singles"
				? singles
				: albumItems;
	const popularTracks = popular.data ?? [];
	const shownPopular = popularExpanded
		? popularTracks
		: popularTracks.slice(0, POPULAR_SHOWN);
	const bio = biography.data;
	const bioIsLong = (bio?.text.length ?? 0) > LONG_BIOGRAPHY_CHARS;

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
							<h1 className="font-semibold text-page-title tracking-tight">
								{name}
							</h1>
							<p className="mt-2 text-caption text-sm">
								{albumCount === 1 ? "1 album" : `${albumCount} albums`}
							</p>
							{/* About sits in the header, clipped to three lines until
							    Read more. */}
							{bio ? (
								<div className="mt-4 max-w-prose">
									<p
										className={`text-foreground text-sm leading-relaxed ${
											bioIsLong && !bioExpanded ? "line-clamp-3" : ""
										}`}
									>
										{bio.text}
									</p>
									<div className="mt-2 flex items-center gap-4 text-xs">
										{bioIsLong ? (
											<button
												type="button"
												aria-expanded={bioExpanded}
												onClick={() => setBioExpanded((value) => !value)}
												className="font-medium text-heading hover:underline"
											>
												{bioExpanded ? "Show less" : "Read more"}
											</button>
										) : null}
										{/* Wikipedia's CC BY-SA licence asks for the source with
										    the text. */}
										<a
											href={bio.sourceUrl}
											target="_blank"
											rel="noopener noreferrer"
											onClick={openExternalLink(bio.sourceUrl)}
											className="text-caption hover:text-heading"
										>
											From Wikipedia, CC BY-SA
										</a>
									</div>
								</div>
							) : null}
						</div>
					</div>
				</div>
			</CoverWash>

			{popularTracks.length > 0 ? (
				<section className="mt-6">
					<h2 className="mb-4 font-semibold text-heading text-xl">Popular</h2>
					<TrackList
						tracks={shownPopular}
						contextTracks={popularTracks}
						playMode="double"
						numbering="list"
						showFavorite
						compact
					/>
					{popularTracks.length > POPULAR_SHOWN ? (
						<button
							type="button"
							aria-expanded={popularExpanded}
							onClick={() => setPopularExpanded((value) => !value)}
							className="mt-3 font-medium text-caption text-sm hover:text-heading"
						>
							{popularExpanded ? "Show less" : "See more"}
						</button>
					) : null}
				</section>
			) : null}

			<section className="mt-12">
				<h2 className="mb-4 font-semibold text-heading text-xl">Discography</h2>
				{filters.length > 0 ? (
					<div className="mb-4 flex flex-wrap gap-2">
						{filters.map((option) => (
							<Button
								key={option.id}
								type="button"
								size="sm"
								variant={filter === option.id ? "default" : "secondary"}
								aria-pressed={filter === option.id}
								onClick={() => setFilter(option.id)}
							>
								{option.label}
							</Button>
						))}
					</div>
				) : null}
				{albums.isLoading ? (
					<p className="text-foreground text-sm">Loading albums…</p>
				) : shownAlbums.length > 0 ? (
					<AlbumGrid albums={shownAlbums} />
				) : (
					<p className="text-caption text-sm">
						No albums under this artist's name.
					</p>
				)}
			</section>
		</DetailPageShell>
	);
}
