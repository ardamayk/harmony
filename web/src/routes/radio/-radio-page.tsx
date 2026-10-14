import type { RadioNowPlaying, RadioStation } from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Headphones, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { useState } from "react";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionPageContainer,
} from "#/components/collection-grid-layout";
import { PageHeader, PageShell } from "#/components/page-layout";
import { Button } from "#/components/ui/button";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import { Input } from "#/components/ui/input";
import { apiClient } from "#/lib/api";
import {
	HEADER_SEARCH_CONTAINER_CLASS,
	HEADER_SEARCH_INPUT_CLASS,
} from "#/lib/page-layout-classes";
import { radioQueryKeys } from "#/lib/radio-query-keys";
import { matchesLocalStationFilter } from "#/lib/radio-station-filter";
import { cn } from "#/lib/utils";

export function RadioPage() {
	const queryClient = useQueryClient();
	const { playRadioStation, currentRadioStation } = usePlayback();
	const [localFilter, setLocalFilter] = useState("");

	const stations = useQuery({
		queryKey: radioQueryKeys.stations,
		queryFn: () => apiClient.listRadioStations(),
	});

	const updateStation = useMutation({
		mutationFn: ({
			stationId,
			...patch
		}: {
			stationId: string;
			name?: string;
			streamUrl?: string;
		}) => apiClient.patchRadioStation(stationId, patch),
		onSuccess: async () => {
			await queryClient.invalidateQueries({
				queryKey: radioQueryKeys.stations,
			});
		},
	});

	const deleteStation = useMutation({
		mutationFn: (stationId: string) => apiClient.deleteRadioStation(stationId),
		onSuccess: async () => {
			await queryClient.invalidateQueries({
				queryKey: radioQueryKeys.stations,
			});
		},
	});

	const stationItems = stations.data?.items ?? [];
	const filteredStations = stationItems.filter((station) =>
		matchesLocalStationFilter(station, localFilter),
	);
	const isStationMutating = updateStation.isPending || deleteStation.isPending;

	return (
		<PageShell
			testId="radio-page-shell"
			contentTestId="radio-page-content"
			header={
				<PageHeader
					title="Radio Stations"
					innerClassName={COLLECTION_PAGE_CONTAINER_CLASS}
					actions={
						<>
							<div className={HEADER_SEARCH_CONTAINER_CLASS}>
								<Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-3 size-4 text-caption" />
								<Input
									className={`${HEADER_SEARCH_INPUT_CLASS} border-border text-player-foreground placeholder:text-player-foreground/55`}
									value={localFilter}
									onChange={(event) => setLocalFilter(event.target.value)}
									placeholder="Search stations..."
									type="search"
								/>
							</div>
							<Button
								aria-label="Add radio station"
								asChild
								className="size-10 rounded-xl bg-[var(--player-control-primary)] text-[var(--player-control-primary-foreground)] hover:opacity-90"
								size="icon"
							>
								<Link to="/radio/discover">
									<Plus className="size-5" />
								</Link>
							</Button>
						</>
					}
				/>
			}
		>
			<CollectionPageContainer className="flex min-h-0 flex-col gap-4">
				<div className="flex items-center justify-between gap-4">
					<h2 className="font-semibold text-heading text-xl">Your Stations</h2>
				</div>

				{stations.isLoading ? (
					<p className="text-foreground text-sm">Loading radio stations...</p>
				) : null}
				{stations.isError ? (
					<p className="text-destructive text-sm">
						Failed to load radio stations
					</p>
				) : null}
				{!stations.isLoading && stationItems.length === 0 ? (
					<div className="rounded-lg border border-dashed border-border p-6 text-caption text-sm">
						No saved stations yet.
					</div>
				) : null}
				{!stations.isLoading &&
				stationItems.length > 0 &&
				filteredStations.length === 0 ? (
					<div className="rounded-lg border border-dashed border-border p-6 text-caption text-sm">
						No saved stations match this filter.
					</div>
				) : null}

				{filteredStations.length > 0 ? (
					<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
						{filteredStations.map((station) => (
							<StationCard
								key={station.id}
								station={station}
								isActive={currentRadioStation?.id === station.id}
								isMutating={isStationMutating}
								onPlay={() => void playRadioStation(station)}
								onUpdate={(patch) =>
									updateStation.mutate({ stationId: station.id, ...patch })
								}
								onDelete={() => deleteStation.mutate(station.id)}
							/>
						))}
					</div>
				) : null}
			</CollectionPageContainer>
		</PageShell>
	);
}

function StationCard({
	station,
	isActive,
	isMutating,
	onPlay,
	onUpdate,
	onDelete,
}: {
	station: RadioStation;
	isActive: boolean;
	isMutating: boolean;
	onPlay: () => void;
	onUpdate: (patch: { name: string; streamUrl: string }) => void;
	onDelete: () => void;
}) {
	const [isEditing, setIsEditing] = useState(false);
	const [isContextMenuOpen, setIsContextMenuOpen] = useState(false);
	const [editName, setEditName] = useState(station.name);
	const [editStreamUrl, setEditStreamUrl] = useState(station.streamUrl);
	const canSaveEdit = Boolean(editName.trim() && editStreamUrl.trim());
	const canEditStation = station.source === "manual";
	const tags = station.tags.slice(0, 2);
	const nowPlaying = useQuery({
		queryKey: radioQueryKeys.nowPlaying(station.id),
		queryFn: () => apiClient.getRadioNowPlaying(station.id),
		refetchOnWindowFocus: false,
		refetchInterval: false,
	});
	const nowPlayingLabel = formatNowPlaying(nowPlaying.data);
	const CardElement = isEditing ? "article" : "button";

	return (
		<ContextMenu open={isContextMenuOpen} onOpenChange={setIsContextMenuOpen}>
			<ContextMenuTrigger asChild>
				<CardElement
					aria-label={isEditing ? undefined : `Play ${station.name}`}
					data-testid={`radio-station-card-${station.id}`}
					className={cn(
						"flex min-w-0 items-start gap-3 rounded-xl border border-border bg-card/45 p-3 text-left transition duration-300 ease-out hover:-translate-y-1 hover:bg-card/65 hover:shadow-lg",
						!isEditing &&
							"cursor-pointer outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
						isActive &&
							"border-border bg-[var(--player)] text-player-foreground shadow-lg",
					)}
					onClick={isEditing ? undefined : onPlay}
					type={isEditing ? undefined : "button"}
				>
					<div className="shrink-0 rounded-lg">
						<StationArtwork
							faviconUrl={station.faviconUrl}
							name={station.name}
						/>
					</div>

					<div className="min-w-0 flex-1">
						{isEditing ? (
							<div className="grid gap-2 sm:grid-cols-2">
								<label
									className="flex min-w-0 flex-col gap-1"
									htmlFor={`${station.id}-edit-radio-name`}
								>
									<span className="font-medium text-caption text-xs">Name</span>
									<Input
										id={`${station.id}-edit-radio-name`}
										value={editName}
										onChange={(event) => setEditName(event.target.value)}
									/>
								</label>
								<label
									className="flex min-w-0 flex-col gap-1"
									htmlFor={`${station.id}-edit-radio-stream-url`}
								>
									<span className="font-medium text-caption text-xs">
										Stream URL
									</span>
									<Input
										id={`${station.id}-edit-radio-stream-url`}
										value={editStreamUrl}
										onChange={(event) => setEditStreamUrl(event.target.value)}
										type="url"
									/>
								</label>
							</div>
						) : (
							<>
								<div className="flex min-w-0 items-center gap-1.5">
									<p
										className={cn(
											"truncate font-semibold text-heading text-sm",
											isActive && "text-[var(--player-control-primary)]",
										)}
										title={station.name}
									>
										{station.name}
									</p>
									{isActive ? <EqualizerBars /> : null}
								</div>
								{tags.length > 0 ? (
									<div className="mt-1.5 flex min-w-0 flex-wrap gap-1.5">
										{tags.map((tag) => (
											<span
												key={tag}
												className="rounded-full border border-border px-2 py-0.5 text-caption text-xs"
											>
												{tag}
											</span>
										))}
									</div>
								) : null}
								{nowPlayingLabel ? (
									<p className="mt-2 truncate text-foreground text-xs">
										{nowPlayingLabel}
									</p>
								) : null}
							</>
						)}
					</div>

					{isEditing ? (
						<div className="flex shrink-0 items-center gap-2">
							<Button
								size="sm"
								disabled={!canSaveEdit || isMutating}
								onClick={() => {
									onUpdate({
										name: editName.trim(),
										streamUrl: editStreamUrl.trim(),
									});
									setIsEditing(false);
								}}
							>
								Save
							</Button>
							<Button
								size="sm"
								variant="outline"
								disabled={isMutating}
								onClick={() => {
									setEditName(station.name);
									setEditStreamUrl(station.streamUrl);
									setIsEditing(false);
								}}
							>
								Cancel
							</Button>
						</div>
					) : null}
				</CardElement>
			</ContextMenuTrigger>
			<ContextMenuContent>
				<ContextMenuItem asChild>
					<Link to="/radio/$stationId" params={{ stationId: station.id }}>
						Details
					</Link>
				</ContextMenuItem>
				{canEditStation ? (
					<ContextMenuItem
						disabled={isMutating}
						onSelect={() => {
							setIsContextMenuOpen(false);
							setIsEditing(true);
						}}
					>
						<Pencil className="size-4" />
						Edit station
					</ContextMenuItem>
				) : null}
				<ContextMenuItem
					disabled={isMutating}
					variant="destructive"
					onSelect={() => {
						setIsContextMenuOpen(false);
						onDelete();
					}}
				>
					<Trash2 className="size-4" />
					Delete station
				</ContextMenuItem>
			</ContextMenuContent>
		</ContextMenu>
	);
}

function EqualizerBars() {
	return (
		<span className="inline-flex h-4 items-end gap-0.5 text-[var(--player-control-primary)]">
			<span className="h-2 w-0.5 rounded-full bg-current" />
			<span className="h-4 w-0.5 rounded-full bg-current" />
			<span className="h-3 w-0.5 rounded-full bg-current" />
		</span>
	);
}

function StationArtwork({
	faviconUrl,
	name,
}: {
	faviconUrl?: string;
	name: string;
}) {
	const [hasImageError, setHasImageError] = useState(false);
	return faviconUrl && !hasImageError ? (
		<img
			alt={name}
			className="h-20 w-20 shrink-0 rounded-lg border border-border bg-muted object-cover"
			src={faviconUrl}
			onError={() => setHasImageError(true)}
		/>
	) : (
		<div
			aria-hidden
			className="flex h-20 w-20 shrink-0 items-center justify-center rounded-lg border border-border bg-[var(--player-artwork)] text-[var(--player-control-primary)]"
		>
			<Headphones className="size-7" />
		</div>
	);
}

function formatNowPlaying(nowPlaying?: RadioNowPlaying): string | null {
	if (!nowPlaying) return null;
	const artist = nowPlaying.artist?.trim();
	const title = nowPlaying.title?.trim();
	if (artist && title) return `${artist} - ${title}`;
	return nowPlaying.raw?.trim() || title || artist || null;
}
