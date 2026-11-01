import { TRACK_WAVEFORM_CAPABILITY } from "@repo/api-client";
import {
	AppShell,
	defaultPlayback,
	defaultPreferences,
	LayoutProvider,
	ListenTogetherProvider,
	PlaybackProvider,
	PlayerBar,
	QueueRowMenuProvider,
	useListenTogether,
	usePlayback,
	usePhoneShell,
} from "@repo/ui";
import { useHiddenFromGuest } from "@repo/ui/guest-access";
import type { QueryClient } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
	createRootRouteWithContext,
	Outlet,
	useNavigate,
} from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { GuestBar } from "#/components/guest-bar";
import { ImportSessionProvider } from "#/components/import-session-provider";
import { LibrarySearchDialog } from "#/components/library-search-dialog";
import { ListenTogetherGate } from "#/components/listen-together-gate";
import { QueueTrackMenu } from "#/components/queue-track-menu";
import { RootErrorComponent } from "#/components/root-error";
import { ThemeSync } from "#/components/theme-sync";
import { isDesktopClient } from "#/desktop/bridge";
import { SidebarPlaylists } from "#/components/sidebar-playlists";
import { DesktopConnectionGate } from "#/desktop/DesktopConnectionGate";
import { DesktopWindowFrame } from "#/desktop/DesktopWindowFrame";
import { desktopWindowControls } from "#/desktop/window-controls";
import { useFavoriteRadioStations } from "#/hooks/use-favorite-radio-stations";
import { useFavoriteTracks } from "#/hooks/use-favorite-tracks";
import {
	useExternalSourceName,
	useServerCapability,
} from "#/hooks/use-server-capability";
import { useFirstLoad } from "#/hooks/use-first-load";
import { apiClient } from "#/lib/api";
import {
	readGuestPreferences,
	writeGuestPreferences,
} from "#/lib/guest-preferences";
import { createPlaybackApi } from "#/lib/playback-api";
import { preferencesPatch } from "#/lib/preferences-patch";
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
	const hideFavorites = useHiddenFromGuest("favorites");
	const hideRadio = useHiddenFromGuest("radio");
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
			onToggleFavorite={hideFavorites ? undefined : toggleFavorite}
			isCurrentStationFavorite={
				currentRadioStation ? isStationFavorite(currentRadioStation.id) : false
			}
			onToggleStationFavorite={hideRadio ? undefined : toggleStationFavorite}
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
	return (
		<ListenTogetherProvider
			api={apiClient}
			engine={getSharedPlaybackEngine()}
			// The core refuses a Guest Listener, so a Desktop Client is the Owner's,
			// and a saved Music Server that is down still opens on its saved lists.
			fallbackRole={isDesktopClient() ? "owner" : undefined}
		>
			<ListenTogetherGate>
				<AppLayout />
			</ListenTogetherGate>
		</ListenTogetherProvider>
	);
}

function AppLayout() {
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const [searchOpen, setSearchOpen] = useState(false);
	const api = useMemo(() => createPlaybackApi(queryClient), [queryClient]);
	const playbackEngine = getSharedPlaybackEngine();
	const { role } = useListenTogether();
	const isPhone = usePhoneShell();
	// Only the Owner has Preferences on the Music Server. A Guest's, or those of
	// a visitor about to join, stay on their device (ADR 0037).
	const hasServerPreferences = role === "owner";
	const preferences = useQuery({
		queryKey: ["preferences"],
		queryFn: () => apiClient.getPreferences(),
		enabled: hasServerPreferences,
		// The window waits on this, so a Music Server that is down must not hold
		// it through retries. A later answer reaches the layout as it stands.
		retry: isDesktopClient() ? false : undefined,
	});

	const patchPreferences = useMutation({
		mutationFn: apiClient.patchPreferences,
		onSuccess: (data) => {
			queryClient.setQueryData(["preferences"], data);
		},
	});

	const isFirstLoad = useFirstLoad(role === "loading" || preferences.isLoading);

	if (isFirstLoad) {
		return <div className="p-8">Loading…</div>;
	}

	const initial = hasServerPreferences
		? (preferences.data ?? defaultPreferences)
		: readGuestPreferences();

	return (
		<LayoutProvider
			initialPreferences={initial}
			loadedPreferences={hasServerPreferences ? preferences.data : undefined}
			onPreferencesChange={(prefs) => {
				if (!hasServerPreferences) writeGuestPreferences(prefs);
				// Saving the stand-in defaults would replace the Owner's real ones.
				else if (preferences.data) {
					patchPreferences.mutate(preferencesPatch(prefs, isPhone));
				}
			}}
		>
			<div className="flex h-full min-h-0 flex-1 flex-col overflow-x-auto overflow-y-hidden">
				<ThemeSync />
				<PlaybackProvider
					api={api}
					engine={playbackEngine}
					autoSkipOnErrorSeconds={
						// Older servers answer without a playback section.
						initial.playback?.autoSkipOnErrorSeconds ??
						defaultPlayback.autoSkipOnErrorSeconds
					}
				>
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
								sidebarPlaylists={<SidebarPlaylists />}
								onSearch={() => setSearchOpen(true)}
								windowControls={
									isDesktopClient() ? desktopWindowControls : undefined
								}
							>
								<GuestBar />
								<Outlet />
								<LibrarySearchDialog
									open={searchOpen}
									onOpenChange={setSearchOpen}
								/>
							</AppShell>
						</QueueRowMenuProvider>
					</ImportSessionProvider>
				</PlaybackProvider>
			</div>
		</LayoutProvider>
	);
}
