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
		<ListenTogetherProvider api={apiClient} engine={getSharedPlaybackEngine()}>
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
	});

	const patchPreferences = useMutation({
		mutationFn: apiClient.patchPreferences,
		onSuccess: (data) => {
			queryClient.setQueryData(["preferences"], data);
		},
	});

	if (role === "loading" || preferences.isLoading) {
		return <div className="p-8">Loading…</div>;
	}

	const initial = hasServerPreferences
		? (preferences.data ?? defaultPreferences)
		: readGuestPreferences();

	return (
		<LayoutProvider
			initialPreferences={initial}
			onPreferencesChange={(prefs) => {
				if (hasServerPreferences) {
					patchPreferences.mutate(preferencesPatch(prefs, isPhone));
				} else writeGuestPreferences(prefs);
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
