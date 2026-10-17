import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";
import { CollectionCoverCardStack } from "#/components/collection-cover-strip";
import {
	CollectionGrid,
	COLLECTION_PAGE_CONTAINER_CLASS,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";
import {
	collectGenres,
	fetchGenreTracks,
	GENRE_SOURCE_QUERY_KEY,
} from "#/lib/collect-genres";
import { cn } from "#/lib/utils";

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
							className={cn(
								"group relative aspect-square overflow-hidden rounded-md border border-border bg-card",
								CARD_INTERACTION_CLASS,
							)}
						>
							<CollectionCoverCardStack
								tracks={genre.tracks}
								seed={`genre:${genre.name}`}
							/>
							<div
								data-genre-card-overlay
								className="absolute inset-x-0 bottom-0 z-50 bg-gradient-to-t from-background/95 via-background/70 to-transparent px-3 pt-12 pb-3 text-foreground"
							>
								<p className="truncate font-semibold text-heading text-sm leading-tight tracking-tight">
									{genre.name}
								</p>
								<p className="mt-0.5 truncate text-xs text-caption leading-tight">
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
