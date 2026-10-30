import { useQuery } from "@tanstack/react-query";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";
import { libraryQueryKeys } from "#/lib/library-query-keys";

export function TracksPage({
	artistId = "",
	search = "",
	onClearFilters,
}: {
	artistId?: string;
	search?: string;
	onClearFilters?: () => void;
} = {}) {
	const tracks = useTrackLibrary(search, artistId);

	return (
		<PageShell
			testId="tracks-page-shell"
			contentTestId="tracks-page-content"
			header={
				<PageHeader
					title="Tracks"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
				/>
			}
		>
			<CollectionPageContainer className="space-y-6">
				{artistId || search ? (
					<div className="flex items-center justify-between gap-3">
						<p className="text-caption text-sm">
							{tracks.tracks.data
								? `Showing ${tracks.items.length} of ${tracks.tracks.data.total} tracks`
								: "Filtered tracks"}
						</p>
						<Button variant="outline" onClick={onClearFilters}>
							Clear filters
						</Button>
					</div>
				) : null}
				<TrackResults {...tracks} filtered={Boolean(artistId || search)} />
			</CollectionPageContainer>
		</PageShell>
	);
}

function useTrackLibrary(search: string, artistId: string) {
	const tracks = useQuery({
		queryKey: libraryQueryKeys.tracks(search, artistId),
		queryFn: () =>
			apiClient.listTracks({
				limit: 200,
				q: search || undefined,
				artistId: artistId || undefined,
			}),
	});
	return { tracks, items: tracks.data?.items ?? [] };
}

function TrackResults({
	tracks,
	items,
	filtered,
}: ReturnType<typeof useTrackLibrary> & { filtered: boolean }) {
	if (tracks.isLoading && items.length === 0)
		return <p className="text-foreground text-sm">Loading tracks…</p>;
	if (tracks.isError && items.length === 0)
		return <p className="text-destructive text-sm">Failed to load tracks</p>;
	if (items.length === 0)
		return (
			<p className="text-foreground text-sm">
				{filtered ? "No tracks match these filters." : "No tracks yet."}
			</p>
		);
	return (
		<TrackList tracks={items} showFavorite showMeta compact numbering="list" />
	);
}
