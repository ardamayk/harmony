import type { Album } from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import { AlbumShelf } from "#/components/album-shelf";
import {
	ExternalLinkButton,
	openExternalLink,
} from "#/components/external-link-button";
import { CoverWash } from "#/components/cover-wash";
import { DetailPageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { useTrackPlayCounts } from "#/hooks/use-track-play-counts";
import { Button } from "#/components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "#/components/ui/dialog";
import { getArtistExternalLinks } from "#/lib/album-external-links";
import { apiClient } from "#/lib/api";
import { sortAlbumsByYear } from "#/lib/sort-albums";
import { cn } from "#/lib/utils";
import { isGone } from "#/lib/query-persistence";

// Albums carry no release type, so the track count decides: a single or an
// EP has at most six tracks, as the stores count them.
const MAX_SINGLE_OR_EP_TRACKS = 6;
// Popular Tracks shown at first, and after See more.
const POPULAR_SHOWN = 5;
const POPULAR_MORE = 10;
// A summary this long is clipped to two lines, all of it behind Read more.
const LONG_BIOGRAPHY_CHARS = 240;

type DiscographyFilter = "all" | "albums" | "singles";

function isSingleOrEp(album: Album) {
	return (
		album.trackCount != null &&
		album.trackCount > 0 &&
		album.trackCount <= MAX_SINGLE_OR_EP_TRACKS
	);
}

/** The Music Server's largest Album page. */
const ALBUM_PAGE = 500;

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
		// Every page: a compilation Artist can have far more Albums than one.
		queryFn: async () => {
			const first = await apiClient.listAlbums({ artistId, limit: ALBUM_PAGE });
			const items = [...first.items];
			while (items.length < first.total) {
				const page = await apiClient.listAlbums({
					artistId,
					limit: ALBUM_PAGE,
					offset: items.length,
				});
				if (page.items.length === 0) break;
				items.push(...page.items);
			}
			return { ...first, items };
		},
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
	const playCounts = useTrackPlayCounts(popular.data ?? []);
	const [bioOpen, setBioOpen] = useState(false);
	// The photo URL that failed to load; a 404 means Deezer had none.
	const [failedPhotoUrl, setFailedPhotoUrl] = useState<string | null>(null);
	const [filter, setFilter] = useState<DiscographyFilter>("all");

	if (artist.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading artist…</div>;
	}
	if (!artist.data || isGone(artist.error)) {
		return <div className="p-6 text-destructive text-sm">Artist not found</div>;
	}

	const { name, albumCount = 0 } = artist.data;
	const photoUrl = apiClient.getArtistPhotoUrl(artistId);
	const photoShown = failedPhotoUrl !== photoUrl;
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
	const discographyHeading = (
		<div className="flex items-baseline gap-3">
			<h2 className="font-semibold text-heading text-xl">Discography</h2>
			<p className="text-caption text-sm">
				{albumCount === 1 ? "1 album" : `${albumCount} albums`}
			</p>
		</div>
	);
	const bio = biography.data;
	const bioIsLong = (bio?.text.length ?? 0) > LONG_BIOGRAPHY_CHARS;

	return (
		<DetailPageShell testId="artist-detail-content">
			{/* Deezer's photos are square, so the banner's backdrop is the whole
			    photo at its full height on the right, melting into the page
			    (Tidal); name and About sit on its left. With no photo the band
			    stays, holding the name. */}
			<CoverWash
				cacheKey={`artist:${artistId}`}
				imageUrl={photoUrl}
				backdropUrl={photoShown ? photoUrl : undefined}
				backdropPlacement="end"
				className={cn(
					"relative flex flex-col justify-end overflow-hidden",
					photoShown && "min-h-artist-banner",
				)}
			>
				{photoShown ? (
					// Not drawn: it only says whether the backdrop has a photo.
					<img
						src={photoUrl}
						alt=""
						hidden
						onError={() => setFailedPhotoUrl(photoUrl)}
					/>
				) : null}
				<div className="relative flex min-w-0 flex-col">
					<h1 className="font-semibold text-page-title tracking-tight">
						{name}
					</h1>
					{/* About, clipped to two lines; Read more opens all of it in a
					    dialog, so the header never moves. */}
					{bio ? (
						<p
							className={cn(
								"mt-4 max-w-2xl text-foreground text-sm leading-relaxed",
								bioIsLong && "line-clamp-2",
							)}
						>
							{bio.text}
						</p>
					) : null}
					<div className="mt-3 flex max-w-2xl flex-wrap items-center gap-x-4 gap-y-2 text-xs">
						{bio && bioIsLong ? (
							<button
								type="button"
								onClick={() => setBioOpen(true)}
								className="font-medium text-heading hover:underline"
							>
								Read more
							</button>
						) : null}
						{bio ? <WikipediaSource href={bio.sourceUrl} /> : null}
						<div className="ml-auto flex gap-1">
							{getArtistExternalLinks(name).map((link) => (
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
					{bio ? (
						<Dialog open={bioOpen} onOpenChange={setBioOpen}>
							<DialogContent className="flex max-h-dialog-height flex-col">
								<DialogTitle>About {name}</DialogTitle>
								<div className="mt-4 min-h-0 overflow-y-auto">
									<p className="whitespace-pre-line text-foreground text-sm leading-relaxed">
										{bio.text}
									</p>
								</div>
								<p className="mt-4 text-xs">
									<WikipediaSource href={bio.sourceUrl} />
								</p>
							</DialogContent>
						</Dialog>
					) : null}
				</div>
			</CoverWash>

			{popularTracks.length > 0 ? (
				<section className="mt-6">
					<h2 className="mb-4 font-semibold text-heading text-xl">Popular</h2>
					<TrackList
						tracks={shownPopular}
						contextTracks={popularTracks}
						numbering="list"
						showFavorite
						compact
						playCounts={playCounts}
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
				{albums.isLoading || shownAlbums.length === 0 ? (
					<>
						{discographyHeading}
						<p
							className={cn(
								"text-sm",
								albums.isLoading ? "text-foreground" : "text-caption",
							)}
						>
							{albums.isLoading
								? "Loading albums…"
								: "No albums under this artist's name."}
						</p>
					</>
				) : (
					<AlbumShelf
						heading={discographyHeading}
						filters={
							filters.length > 0 ? (
								<div className="mb-2 flex flex-wrap gap-2">
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
							) : null
						}
						albums={shownAlbums}
					/>
				)}
			</section>
		</DetailPageShell>
	);
}

/** Wikipedia's CC BY-SA licence asks for the source with the text. */
function WikipediaSource({ href }: { href: string }) {
	return (
		<a
			href={href}
			target="_blank"
			rel="noopener noreferrer"
			onClick={openExternalLink(href)}
			className="text-caption hover:text-heading"
		>
			From Wikipedia, CC BY-SA
		</a>
	);
}
