import { TOP_NAV_ICON_BUTTON_CLASS, TopNavActions } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { Disc3, SlidersHorizontal, TriangleAlert, X } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { useDeferredValue, useMemo, useState } from "react";
import {
	type AlbumFilterState,
	AlbumFilters,
	collectAlbumGenres,
	filterAlbums,
} from "#/components/album-filters";
import { AlbumGrid } from "#/components/album-grid";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionGridSkeleton,
	CollectionGridState,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";

const defaultFilters: AlbumFilterState = {
	albumQuery: "",
	artistId: "all",
	genre: "all",
};

export function AlbumsPage() {
	const [filters, setFilters] = useState<AlbumFilterState>(defaultFilters);
	const [areFiltersOpen, setAreFiltersOpen] = useState(false);
	const deferredAlbumQuery = useDeferredValue(filters.albumQuery.trim());

	const artists = useQuery({
		queryKey: ["library", "artists", "all"],
		queryFn: () => apiClient.listArtists({ limit: 500 }),
		staleTime: 60_000,
	});

	const genreSource = useQuery({
		queryKey: ["library", "albums", "genre-source"],
		queryFn: () => apiClient.listAlbums({ limit: 500 }),
		staleTime: 60_000,
	});

	const albums = useQuery({
		queryKey: ["library", "albums", deferredAlbumQuery, filters.artistId],
		queryFn: () =>
			apiClient.listAlbums({
				limit: 500,
				q: deferredAlbumQuery || undefined,
				artistId:
					filters.artistId && filters.artistId !== "all"
						? filters.artistId
						: undefined,
			}),
	});

	const genreOptions = useMemo(
		() => collectAlbumGenres(genreSource.data?.items ?? []),
		[genreSource.data?.items],
	);

	const visibleAlbums = useMemo(
		() => filterAlbums(albums.data?.items ?? [], filters),
		[albums.data?.items, filters],
	);
	const isInitialLoading =
		(albums.isLoading && !albums.data) ||
		(artists.isLoading && !artists.data) ||
		(genreSource.isLoading && !genreSource.data);
	const hasError =
		(albums.isError && !albums.data) ||
		(artists.isError && !artists.data) ||
		(genreSource.isError && !genreSource.data);
	const isFetching =
		albums.isFetching || artists.isFetching || genreSource.isFetching;
	const isRetrying =
		(albums.isError && albums.isFetching) ||
		(artists.isError && artists.isFetching) ||
		(genreSource.isError && genreSource.isFetching);
	const hasActiveFilters =
		filters.albumQuery.trim().length > 0 ||
		filters.artistId !== "all" ||
		filters.genre !== "all";

	const handleRetry = () => {
		void Promise.all([
			albums.refetch(),
			artists.refetch(),
			genreSource.refetch(),
		]);
	};

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
			<TopNavActions>
				<button
					type="button"
					aria-label="Filters"
					title="Filters"
					aria-pressed={areFiltersOpen || hasActiveFilters}
					className={TOP_NAV_ICON_BUTTON_CLASS}
					onClick={() => setAreFiltersOpen(true)}
				>
					<SlidersHorizontal className="size-4" />
				</button>
			</TopNavActions>
			<CollectionPageContainer aria-busy={isFetching || undefined}>
				<AlbumFilterDrawer
					isOpen={areFiltersOpen}
					artists={artists.data?.items ?? []}
					genreOptions={genreOptions}
					filters={filters}
					resultCount={visibleAlbums.length}
					onOpenChange={setAreFiltersOpen}
					onFiltersChange={setFilters}
				/>

				{hasError ? (
					<CollectionGridState
						kind="error"
						icon={<TriangleAlert aria-hidden />}
						title="Unable to load albums"
						description="Check your connection and try again."
						onRetry={handleRetry}
						isRetrying={isRetrying}
					/>
				) : isInitialLoading ? (
					<CollectionGridSkeleton label="Loading albums" />
				) : visibleAlbums.length === 0 ? (
					<CollectionGridState
						kind="empty"
						icon={<Disc3 aria-hidden />}
						title={
							hasActiveFilters
								? "No albums match your filters"
								: "No albums yet"
						}
						description={
							hasActiveFilters
								? "Try adjusting your search or filters."
								: "Import music to get started."
						}
					/>
				) : (
					<AlbumGrid albums={visibleAlbums} />
				)}
			</CollectionPageContainer>
		</PageShell>
	);
}

function AlbumFilterDrawer({
	isOpen,
	artists,
	genreOptions,
	filters,
	resultCount,
	onOpenChange,
	onFiltersChange,
}: {
	isOpen: boolean;
	artists: Parameters<typeof AlbumFilters>[0]["artists"];
	genreOptions: string[];
	filters: AlbumFilterState;
	resultCount: number;
	onOpenChange: (isOpen: boolean) => void;
	onFiltersChange: (next: AlbumFilterState) => void;
}) {
	return (
		<DialogPrimitive.Root open={isOpen} onOpenChange={onOpenChange}>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-background/50 backdrop-blur-sm" />
				<DialogPrimitive.Content
					aria-describedby="album-filter-description"
					className="fixed top-0 right-0 z-50 flex h-dvh w-sm flex-col gap-6 border-border border-l bg-background p-6 shadow-xl outline-none"
				>
					<div className="flex items-start justify-between gap-4">
						<div className="min-w-0">
							<DialogPrimitive.Title className="font-semibold text-heading text-xl">
								Album filters
							</DialogPrimitive.Title>
							<DialogPrimitive.Description
								id="album-filter-description"
								className="mt-1 text-caption text-sm"
							>
								Selections apply immediately.
							</DialogPrimitive.Description>
						</div>
						<DialogPrimitive.Close asChild>
							<Button type="button" variant="ghost" size="icon">
								<X className="size-4" />
								<span className="sr-only">Close</span>
							</Button>
						</DialogPrimitive.Close>
					</div>

					<div className="min-h-0 flex-1 overflow-y-auto scrollbar-hidden">
						<AlbumFilters
							artists={artists}
							genreOptions={genreOptions}
							filters={filters}
							onFiltersChange={onFiltersChange}
							resultCount={resultCount}
							showSearch={false}
						/>
					</div>
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}
