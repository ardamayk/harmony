import { useQuery } from "@tanstack/react-query";
import { Disc3, TriangleAlert } from "lucide-react";
import { AlbumGrid } from "#/components/album-grid";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionGridSkeleton,
	CollectionGridState,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { apiClient } from "#/lib/api";

export function AlbumsPage() {
	const albums = useQuery({
		queryKey: ["library", "albums", "", "all"],
		queryFn: () => apiClient.listAlbums({ limit: 500 }),
	});
	const visibleAlbums = albums.data?.items ?? [];

	return (
		<PageShell
			testId="albums-page-shell"
			contentTestId="albums-page-content"
			header={
				<PageHeader
					title="Albums"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
				/>
			}
		>
			<CollectionPageContainer aria-busy={albums.isFetching || undefined}>
				{albums.isError && !albums.data ? (
					<CollectionGridState
						kind="error"
						icon={<TriangleAlert aria-hidden />}
						title="Unable to load albums"
						description="Check your connection and try again."
						onRetry={() => void albums.refetch()}
						isRetrying={albums.isFetching}
					/>
				) : albums.isLoading ? (
					<CollectionGridSkeleton label="Loading albums" />
				) : visibleAlbums.length === 0 ? (
					<CollectionGridState
						kind="empty"
						icon={<Disc3 aria-hidden />}
						title="No albums yet"
						description="Import music to get started."
					/>
				) : (
					<AlbumGrid albums={visibleAlbums} />
				)}
			</CollectionPageContainer>
		</PageShell>
	);
}
