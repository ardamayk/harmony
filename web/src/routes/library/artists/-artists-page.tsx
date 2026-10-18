import { useQuery } from "@tanstack/react-query";
import { TriangleAlert, Users } from "lucide-react";
import { ArtistGrid } from "#/components/artist-grid";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionGridSkeleton,
	CollectionGridState,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";

export function ArtistsPage({
	initialSearch = "",
}: {
	initialSearch?: string;
}) {
	// Library search routes artist picks here as `?q=`; the page has no
	// search field of its own.
	const search = initialSearch;
	const artists = useQuery({
		queryKey: ["library", "artists", search],
		queryFn: () =>
			apiClient.listArtists({ limit: 100, q: search || undefined }),
	});
	const artistItems = artists.data?.items ?? [];
	const isInitialLoading = artists.isLoading && !artists.data;
	const hasError = artists.isError && !artists.data;

	return (
		<PageShell
			testId="artists-page-shell"
			contentTestId="artists-page-content"
			header={
				<PageHeader
					title="Artists"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
				/>
			}
		>
			<CollectionPageContainer aria-busy={artists.isFetching || undefined}>
				{isInitialLoading ? (
					<CollectionGridSkeleton label="Loading artists" />
				) : hasError ? (
					<CollectionGridState
						kind="error"
						icon={<TriangleAlert aria-hidden />}
						title="Unable to load artists"
						description="Check your connection and try again."
						onRetry={() => void artists.refetch()}
						isRetrying={artists.isFetching}
					/>
				) : artistItems.length === 0 ? (
					<CollectionGridState
						kind="empty"
						icon={<Users aria-hidden />}
						title={
							search.trim() ? "No artists match your search" : "No artists yet"
						}
						description={
							search.trim()
								? "Try adjusting your search."
								: "Import music to get started."
						}
					/>
				) : (
					<ArtistGrid artists={artistItems} />
				)}
			</CollectionPageContainer>
		</PageShell>
	);
}
