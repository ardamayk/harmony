import { TRACK_WAVEFORM_CAPABILITY } from "@repo/api-client";
import {
	AppShell,
	defaultPlayback,
	defaultPreferences,
	LayoutProvider,
	PlaybackProvider,
	PlayerBar,
	QueueRowMenuProvider,
	usePlayback,
} from "@repo/ui";
import type { QueryClient } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
	createRootRouteWithContext,
	Outlet,
	useNavigate,
	useRouterState,
} from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { ImportSessionProvider } from "#/components/import-session-provider";
import { LibrarySearchDialog } from "#/components/library-search-dialog";
import { QueueTrackMenu } from "#/components/queue-track-menu";
import { RootErrorComponent } from "#/components/root-error";
import { ThemeSync } from "#/components/theme-sync";
import { isDesktopClient } from "#/desktop/bridge";
import { DesktopConnectionGate } from "#/desktop/DesktopConnectionGate";
import { DesktopWindowFrame } from "#/desktop/DesktopWindowFrame";
import { toggleMiniPlayer } from "#/desktop/mini-player-bridge";
import { desktopWindowControls } from "#/desktop/window-controls";
import { useFavoriteRadioStations } from "#/hooks/use-favorite-radio-stations";
import { useFavoriteTracks } from "#/hooks/use-favorite-tracks";
import {
	useExternalSourceName,
	useServerCapability,
} from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";
import { isMiniPlayerRoute } from "#/lib/mini-player-route";
import { createPlaybackApi } from "#/lib/playback-api";
import { invalidatePlaylistCache } from "#/lib/playlist-query-cache";
import { getSharedPlaybackEngine } from "#/playback/shared-playback-engine";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()(
	{
		component: RootLayout,
		errorComponent: RootErrorComponent,
	},
);

function PlayerBarWithSync() {
	const queryClient = useQueryClient();
	const { currentTrack, currentRadioStation } = usePlayback();
	const { isFavorite, toggleFavorite } = useFavoriteTracks();
	const { isStationFavorite, toggleStationFavorite } =
		useFavoriteRadioStations();
	const canShowWaveform = useServerCapability(TRACK_WAVEFORM_CAPABILITY);
	const externalSourceName = useExternalSourceName();
	return (
		<PlayerBar
			onPlaylistMutated={() => {
				void invalidatePlaylistCache(queryClient);
			}}
			isCurrentTrackFavorite={
				currentTrack ? isFavorite(currentTrack.id) : false
			}
			onToggleFavorite={toggleFavorite}
			isCurrentStationFavorite={
				currentRadioStation ? isStationFavorite(currentRadioStation.id) : false
			}
			onToggleStationFavorite={toggleStationFavorite}
			onToggleMiniPlayer={
				isDesktopClient() ? () => void toggleMiniPlayer() : undefined
			}
			windowControls={isDesktopClient() ? desktopWindowControls : undefined}
			canShowWaveform={canShowWaveform}
			externalSourceName={externalSourceName}
		/>
	);
}

function RootLayout() {
	return (
		<>
			<DesktopWindowFrame />
			<DesktopConnectionGate>
				<ConnectedRootLayout />
			</DesktopConnectionGate>
		</>
	);
}

function ConnectedRootLayout() {
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const [searchOpen, setSearchOpen] = useState(false);
	const api = useMemo(() => createPlaybackApi(queryClient), [queryClient]);
	const playbackEngine = getSharedPlaybackEngine();
	const isMiniPlayer = useRouterState({
		select: (state) => isMiniPlayerRoute(state.location.pathname),
	});
	const preferences = useQuery({
		queryKey: ["preferences"],
		queryFn: () => apiClient.getPreferences(),
	});

	const patchPreferences = useMutation({
		mutationFn: apiClient.patchPreferences,
		onSuccess: (data) => {
			queryClient.setQueryData(["preferences"], data);
		},
	});

	if (preferences.isLoading) {
		return <div className="p-8">Loading…</div>;
	}

	const initial = preferences.data ?? defaultPreferences;

	return (
		<LayoutProvider
			initialPreferences={initial}
			onPreferencesChange={(prefs) => {
				patchPreferences.mutate(prefs);
			}}
		>
			<div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
				<ThemeSync />
				<PlaybackProvider
					api={api}
					engine={playbackEngine}
					shouldCoordinateQueue={!isMiniPlayer}
					autoSkipOnErrorSeconds={
						// Older servers answer without a playback section.
						initial.playback?.autoSkipOnErrorSeconds ??
						defaultPlayback.autoSkipOnErrorSeconds
					}
				>
					{isMiniPlayer ? (
						// The mini player window renders only the compact player.
						<Outlet />
					) : (
						<ImportSessionProvider>
							<QueueRowMenuProvider
								menu={QueueTrackMenu}
								onNavigate={(href) => {
									void navigate({ to: href }).catch((error) =>
										console.warn("Failed to navigate from queue", {
											href,
											error,
										}),
									);
								}}
							>
								<AppShell
									bottom={<PlayerBarWithSync />}
									onSearch={() => setSearchOpen(true)}
									windowControls={
										isDesktopClient() ? desktopWindowControls : undefined
									}
								>
									<Outlet />
									<LibrarySearchDialog
										open={searchOpen}
										onOpenChange={setSearchOpen}
									/>
								</AppShell>
							</QueueRowMenuProvider>
						</ImportSessionProvider>
					)}
				</PlaybackProvider>
			</div>
		</LayoutProvider>
	);
}
