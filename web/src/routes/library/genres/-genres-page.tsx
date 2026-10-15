import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";
import { CollectionCoverCardStack } from "#/components/collection-cover-strip";
import {
	CollectionGrid,
	COLLECTION_PAGE_CONTAINER_CLASS,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";
import {
	collectGenres,
	fetchGenreTracks,
	GENRE_SOURCE_QUERY_KEY,
} from "#/lib/collect-genres";

export function GenresPage() {
	const tracks = useQuery({
		queryKey: GENRE_SOURCE_QUERY_KEY,
		queryFn: () => fetchGenreTracks(apiClient.listTracks),
		staleTime: 60_000,
	});

	const genres = useMemo(
		() => collectGenres(tracks.data?.items ?? []),
		[tracks.data?.items],
	);

	if (tracks.isLoading) {
		return <div className="p-6 text-foreground text-sm">Loading genres…</div>;
	}

	if (tracks.isError) {
		return (
			<div className="p-6 text-destructive text-sm">Failed to load genres</div>
		);
	}

	return (
		<PageShell
			testId="genres-page-shell"
			contentTestId="genres-page-content"
			header={
				<PageHeader
					title="Genres"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
				/>
			}
		>
			{genres.length === 0 ? (
				<p className="text-foreground text-sm">No tagged genres yet.</p>
			) : (
				<CollectionGrid className={COLLECTION_PAGE_CONTAINER_CLASS}>
					{genres.map((genre) => (
						<Link
							key={genre.name}
							to="/library/genres/$genre"
							params={{ genre: genre.name }}
							className="group relative aspect-square overflow-hidden rounded-md border border-border bg-card transition duration-300 ease-out hover:-translate-y-px hover:border-ring/60 hover:bg-muted/50 hover:shadow-lg focus-visible:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60 focus-visible:ring-offset-2 focus-visible:ring-offset-background motion-reduce:transform-none motion-reduce:transition-none"
						>
							<CollectionCoverCardStack
								tracks={genre.tracks}
								seed={`genre:${genre.name}`}
							/>
							<div
								data-genre-card-overlay
								className="absolute inset-x-0 bottom-0 z-50 bg-gradient-to-t from-background/95 via-background/70 to-transparent px-3 pt-14 pb-2.5 text-foreground"
							>
								<p className="truncate font-semibold text-heading text-sm leading-tight tracking-[-0.012em]">
									{genre.name}
								</p>
								<p className="mt-0.5 truncate text-[11px] text-caption leading-tight tracking-[0.01em]">
									{genre.trackCount} track{genre.trackCount === 1 ? "" : "s"}
								</p>
							</div>
						</Link>
					))}
				</CollectionGrid>
			)}
		</PageShell>
	);
}
