import { MANAGED_IMPORT_CAPABILITY } from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { useManagedImport } from "#/components/import-session-context";
import { PageHeader, PageShell } from "#/components/page-layout";
import { TrackList } from "#/components/track-list";
import { Button } from "#/components/ui/button";
import {
	type ServerCapabilityState,
	useServerCapabilityState,
} from "#/hooks/use-server-capability";
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
	const managedImport = useManagedImport();
	const importCapability = useServerCapabilityState(MANAGED_IMPORT_CAPABILITY);

	return (
		<PageShell
			testId="tracks-page-shell"
			contentTestId="tracks-page-content"
			header={
				<TracksHeader
					onImport={managedImport.open}
					importCapability={importCapability}
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

const IMPORT_UNSUPPORTED_TITLE =
	"This Music Server does not support Managed Import. Update the Music Server to import music.";

function TracksHeader({
	onImport,
	importCapability,
}: {
	onImport: () => void;
	importCapability: ServerCapabilityState;
}) {
	// Gate on the advertised Server Capability (ADR 0006). The control stays
	// usable while the health response is unknown so a slow answer never hides
	// the Import Music action from a compatible Music Server.
	const importUnsupported = importCapability === "missing";
	return (
		<PageHeader
			title="Tracks"
			innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
			actions={
				<Button
					type="button"
					aria-label="Import Music"
					size="icon-lg"
					onClick={onImport}
					disabled={importUnsupported}
					title={importUnsupported ? IMPORT_UNSUPPORTED_TITLE : undefined}
				>
					<Plus className="size-5" />
				</Button>
			}
		/>
	);
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
