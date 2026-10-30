import type { Playlist, RadioStation, Track } from "@repo/api-client";
import { useNavigate } from "@tanstack/react-router";
import {
	Check,
	ChevronRight,
	Download,
	Gauge,
	Heart,
	Info,
	ListEnd,
	ListPlus,
	MoonStar,
	MoreVertical,
	Plus,
	Repeat1,
	X,
} from "lucide-react";
import {
	type CSSProperties,
	type ReactNode,
	useCallback,
	useEffect,
	useEffectEvent,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { parseRecentPlaylistIds } from "../lib/stored-preferences";
import { useFocusTrap } from "../lib/use-focus-trap";
import { cn } from "../lib/utils";
import { useRoomPlayback } from "../listen-together/RoomPlaybackContext";
import { NowPlayingAnnouncer } from "../playback/NowPlayingAnnouncer";
import { usePlayback, usePlaylistLibrary } from "../playback/PlaybackProvider";
import { createFallbackPlaybackTelemetry } from "../playback/telemetry";
import {
	buildTrackDetailRows,
	formatBitDepth,
	formatSampleRate,
	getTrackArtistCredits,
	goToArtistCreditsSearch,
} from "../playback/track-details";
import { useCoverAccent, useRootAccentHue } from "../playback/use-cover-accent";
import { useMute } from "../playback/use-mute";
import { usePlaybackKeyboardShortcuts } from "../playback/use-playback-keyboard-shortcuts";
import { useTrackWaveform } from "../playback/use-track-waveform";
import { useResolvedThemeMode } from "../theme/use-resolved-theme-mode";
import { getQueuePanel } from "../widgets/layout-utils";
import { AlbumArt } from "./AlbumArt";
import { useHiddenFromGuest } from "./guest-access";
import { useLayout } from "./LayoutProvider";
import { LyricsOverlay } from "./LyricsOverlay";
import { NowPlayingView } from "./NowPlayingView";
import type { WindowControls } from "./TopNav";
import { PlaybackErrorBanner } from "./PlaybackErrorBanner";
import { PlaybackSignal } from "./PlaybackSignal";
import {
	PlaybackControls,
	QualityIconFor,
	VolumeAndQueueControls,
} from "./PlayerBarControls";
import { buildQualityDetailRows } from "./QualityDetailsCard";
import { findCurrentQueueIndex } from "./queue-groups";
import { ShortcutHelpOverlay } from "./ShortcutHelpOverlay";
import { UpNextPeek } from "./UpNextPeek";

const PLAYBACK_RATES = [0.75, 1, 1.25, 1.5, 2] as const;
const SLEEP_TIMER_MINUTES = [15, 30, 60] as const;

const RECENT_PLAYLISTS_KEY = "navidrome-recent-playlists";
const RECENT_PLAYLIST_LIMIT = 2;

function readRecentPlaylistIds(): string[] {
	if (typeof window === "undefined") return [];
	try {
		return parseRecentPlaylistIds(
			window.localStorage.getItem(RECENT_PLAYLISTS_KEY),
		);
	} catch {
		return [];
	}
}

function touchRecentPlaylist(playlistId: string) {
	if (typeof window === "undefined") return;
	const recent = readRecentPlaylistIds().filter((id) => id !== playlistId);
	recent.unshift(playlistId);
	window.localStorage.setItem(
		RECENT_PLAYLISTS_KEY,
		JSON.stringify(recent.slice(0, 10)),
	);
}

function formatQualityLabel(
	track: { bitDepth?: number; sampleRateHz?: number } | null,
): string {
	if (!track) return "Quality";
	const bitDepth = formatBitDepth(track.bitDepth);
	const sampleRate = formatSampleRate(track.sampleRateHz);
	if (!bitDepth && !sampleRate) return "Quality";
	return [bitDepth ?? "-", sampleRate ?? "-"].join(" · ");
}

function formatRadioQualityLabel(station: RadioStation | null): string {
	if (!station) return "Quality";
	const parts = [
		station.codec ? station.codec.toUpperCase() : null,
		station.bitrate ? `${station.bitrate} kbps` : null,
	].filter(Boolean);
	return parts.length > 0 ? parts.join(" · ") : "High Quality";
}

function isLosslessFormat(format?: string): boolean {
	return ["flac", "alac", "wav", "aiff", "dsd"].includes(
		format?.toLowerCase() ?? "",
	);
}

type MenuPosition = {
	top: number;
	left: number;
};

export function PlayerBar({
	onPlaylistMutated,
	isCurrentTrackFavorite = false,
	onToggleFavorite,
	isCurrentStationFavorite = false,
	onToggleStationFavorite,
	onToggleMiniPlayer,
	windowControls,
	canShowWaveform = false,
	externalSourceName,
}: {
	onPlaylistMutated?: () => void;
	/** Favorite state of the current Track; the host app owns the Favorites playlist. */
	isCurrentTrackFavorite?: boolean;
	onToggleFavorite?: (trackId: string) => void;
	/** Favorite state of the current saved Radio Station; previews are never favorites. */
	isCurrentStationFavorite?: boolean;
	onToggleStationFavorite?: (stationId: string, isFavorite: boolean) => void;
	/** Desktop only: opens or closes the always-on-top mini player window. */
	onToggleMiniPlayer?: () => void;
	/** Window buttons for a host without a title bar; full-screen views cover the Top Nav's. */
	windowControls?: WindowControls;
	/** Whether the Music Server advertises track-waveform.v1. */
	canShowWaveform?: boolean;
	/** The External Source's site, such as YouTube, shown next to "External". */
	externalSourceName?: string;
} = {}) {
	const navigate = useNavigate();
	const actionsButtonRef = useRef<HTMLButtonElement>(null);
	const playlistSubmenuCloseTimerRef = useRef<number | null>(null);
	const [actionsOpen, setActionsOpen] = useState(false);
	const [menuPosition, setMenuPosition] = useState<MenuPosition | null>(null);
	const [playlistSubmenuOpen, setPlaylistSubmenuOpen] = useState(false);
	const [infoOpen, setInfoOpen] = useState(false);
	const [localLyricsOpen, setLocalLyricsOpen] = useState(false);
	const roomPlayback = useRoomPlayback();
	const room = roomPlayback?.room ?? null;
	const sendRoomCommand = roomPlayback?.sendCommand;
	const isInRoom = room !== null;
	const hidePlaylists = useHiddenFromGuest("playlists");
	// During a Room the lyrics open for everyone (ADR 0037).
	const lyricsOpen = room ? room.lyricsOpen : localLyricsOpen;
	const setLyricsOpen = useCallback(
		(open: boolean) => {
			if (isInRoom) void sendRoomCommand?.({ type: "setLyricsOpen", open });
			else setLocalLyricsOpen(open);
		},
		[isInRoom, sendRoomCommand],
	);
	const [helpOpen, setHelpOpen] = useState(false);
	const [nowPlayingOpen, setNowPlayingOpen] = useState(false);
	const [playlists, setPlaylists] = useState<Playlist[]>([]);
	const [playlistsLoaded, setPlaylistsLoaded] = useState(false);
	const [memberPlaylistIds, setMemberPlaylistIds] = useState<Set<string>>(
		() => new Set(),
	);
	const [playlistQuery, setPlaylistQuery] = useState("");
	const [createPlaylistOpen, setCreatePlaylistOpen] = useState(false);
	const [newPlaylistName, setNewPlaylistName] = useState("");
	const { preferences, togglePanel } = useLayout();
	const queuePanelSide = getQueuePanel(preferences.layout.sidebarPosition);
	const playbackPreferences = preferences.playback;
	const {
		queue,
		outputMode,
		outputDeviceIssue,
		currentTrack,
		currentRadioStation,
		radioNowPlaying,
		currentExternalTrack,
		isPlaying,
		isReconnecting,
		currentTime,
		duration,
		bufferedEnd,
		volume,
		playbackRate,
		isGapless,
		sleepTimer,
		abRepeat,
		shuffleEnabled,
		repeatMode,
		playbackError,
		errorRecovery,
		togglePlay,
		navigatePrevious,
		navigateNext,
		toggleShuffle,
		cycleRepeatMode,
		seek,
		setVolume,
		setPlaybackRate,
		setTransitionFade,
		playNext,
		addToQueue,
		selectExclusiveOutput,
		fallbackToSystemOutput,
		enableAdaptiveSystemRate,
		processingState,
		playbackTelemetry,
		playbackSource,
		getAlbumCoverUrl,
		getExternalTrackArtworkUrl,
		getTrackLyrics,
		getTrackWaveform,
	} = usePlayback();
	const currentQueueIndex = findCurrentQueueIndex(
		queue,
		playbackSource?.type === "track" ? playbackSource.queueItemId : undefined,
		currentTrack?.id,
	);
	const upcomingCount = queue.length - currentQueueIndex - 1;
	const waveform = useTrackWaveform({
		trackId: currentTrack ? currentTrack.id : null,
		enabled: canShowWaveform && playbackPreferences.showWaveform,
		load: getTrackWaveform,
	});
	const { toggleMute } = useMute(volume, setVolume);
	// Playback Preferences are the source of truth for speed and fade; push
	// them into whichever engine is active when they change. The setters are
	// recreated on every session update, so they are effect events: depending
	// on them would re-apply the preference in a loop.
	const applyPlaybackRate = useEffectEvent((rate: number) =>
		setPlaybackRate(rate),
	);
	const applyTransitionFade = useEffectEvent((fadeMs: number) =>
		setTransitionFade(fadeMs),
	);
	useEffect(() => {
		applyPlaybackRate(playbackPreferences.playbackRate);
	}, [playbackPreferences.playbackRate]);
	useEffect(() => {
		applyTransitionFade(playbackPreferences.transitionFadeMs);
	}, [playbackPreferences.transitionFadeMs]);
	const canSeek = Boolean(currentTrack || currentExternalTrack);
	const seekBy = useCallback(
		(delta: number) => {
			if (!canSeek) return;
			const limit = duration > 0 ? duration : Number.POSITIVE_INFINITY;
			seek(Math.min(limit, Math.max(0, currentTime + delta)));
		},
		[canSeek, currentTime, duration, seek],
	);
	const adjustVolume = useCallback(
		(delta: number) => setVolume(Math.min(1, Math.max(0, volume + delta))),
		[setVolume, volume],
	);
	const toggleLyrics = useCallback(() => {
		if (!currentTrack) return;
		setLyricsOpen(!lyricsOpen);
	}, [currentTrack, lyricsOpen, setLyricsOpen]);
	const toggleQueue = useCallback(
		() => togglePanel(queuePanelSide),
		[togglePanel, queuePanelSide],
	);
	const toggleHelp = useCallback(() => setHelpOpen((open) => !open), []);
	usePlaybackKeyboardShortcuts(
		{
			togglePlay,
			navigatePrevious,
			navigateNext,
			seekBy,
			adjustVolume,
			toggleMute,
			toggleLyrics,
			toggleQueue,
			toggleHelp,
			toggleMiniPlayer: onToggleMiniPlayer,
		},
		{
			seekStepSeconds: playbackPreferences.seekStepSeconds,
			seekStepLargeSeconds: playbackPreferences.seekStepLargeSeconds,
		},
	);
	const {
		listPlaylists,
		getPlaylist,
		createPlaylist,
		addPlaylistTrack,
		removePlaylistTrack,
	} = usePlaylistLibrary();

	const updateMenuPosition = useCallback(() => {
		const button = actionsButtonRef.current;
		if (!button) return;
		const rect = button.getBoundingClientRect();
		setMenuPosition({
			top: rect.top - 8,
			left: rect.left,
		});
	}, []);

	useEffect(() => {
		if (!actionsOpen) return;
		updateMenuPosition();
		const handlePointerDown = (event: MouseEvent) => {
			const target = event.target;
			if (!(target instanceof Node)) return;
			if (actionsButtonRef.current?.contains(target)) return;
			const menu = document.getElementById("player-track-actions-menu");
			if (menu?.contains(target)) return;
			setActionsOpen(false);
		};
		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setPlaylistSubmenuOpen(false);
				setActionsOpen(false);
			}
		};
		document.addEventListener("mousedown", handlePointerDown);
		document.addEventListener("keydown", handleKeyDown);
		window.addEventListener("resize", updateMenuPosition);
		window.addEventListener("scroll", updateMenuPosition, true);
		return () => {
			document.removeEventListener("mousedown", handlePointerDown);
			document.removeEventListener("keydown", handleKeyDown);
			window.removeEventListener("resize", updateMenuPosition);
			window.removeEventListener("scroll", updateMenuPosition, true);
		};
	}, [actionsOpen, updateMenuPosition]);

	/** Ids of the playlists that contain the Track. */
	const fetchPlaylistMembership = useCallback(
		async (items: Playlist[], trackId: string) => {
			const details = await Promise.all(
				items.map((playlist) => getPlaylist(playlist.id)),
			);
			return new Set(
				details
					.filter((detail) =>
						detail.tracks.some((entry) => entry.id === trackId),
					)
					.map((detail) => detail.id),
			);
		},
		[getPlaylist],
	);
	/** Shows playlist names as soon as they load, then their membership. */
	const loadPlaylists = async (trackId: string) => {
		const { items } = await listPlaylists();
		setPlaylists(items);
		setMemberPlaylistIds(await fetchPlaylistMembership(items, trackId));
		setPlaylistsLoaded(true);
	};
	const currentTrackId = currentTrack ? currentTrack.id : null;

	// Closing the actions menu (or losing the Track) resets the playlist submenu.
	const isActionsMenuActive = actionsOpen && Boolean(currentTrack);
	const [wasActionsMenuActive, setWasActionsMenuActive] =
		useState(isActionsMenuActive);
	if (isActionsMenuActive !== wasActionsMenuActive) {
		setWasActionsMenuActive(isActionsMenuActive);
		if (!isActionsMenuActive) {
			setPlaylistSubmenuOpen(false);
			setPlaylistQuery("");
			setCreatePlaylistOpen(false);
			setNewPlaylistName("");
			setPlaylistsLoaded(false);
		}
	}

	useEffect(() => {
		if (!isActionsMenuActive || !currentTrackId || hidePlaylists) {
			return undefined;
		}
		let cancelled = false;
		void listPlaylists().then(({ items }) => {
			if (cancelled) return undefined;
			setPlaylists(items);
			return fetchPlaylistMembership(items, currentTrackId).then(
				(memberIds) => {
					if (cancelled) return;
					setMemberPlaylistIds(memberIds);
					setPlaylistsLoaded(true);
				},
			);
		});
		return () => {
			cancelled = true;
		};
	}, [
		isActionsMenuActive,
		currentTrackId,
		hidePlaylists,
		listPlaylists,
		fetchPlaylistMembership,
	]);

	useEffect(() => {
		return () => {
			if (playlistSubmenuCloseTimerRef.current !== null) {
				window.clearTimeout(playlistSubmenuCloseTimerRef.current);
			}
		};
	}, []);

	const isRadioPlaying = Boolean(currentRadioStation);
	const hasPlayableSource = Boolean(
		currentTrack || currentExternalTrack || currentRadioStation,
	);
	const radioTitle =
		radioNowPlaying?.title ??
		radioNowPlaying?.raw ??
		currentRadioStation?.name ??
		null;
	const nowPlayingTitle =
		currentTrack?.title ??
		currentExternalTrack?.title ??
		radioTitle ??
		"Nothing playing";
	const nowPlayingSubtitle = isReconnecting
		? "Reconnecting…"
		: (currentTrack?.artistName ??
			(currentExternalTrack
				? (currentExternalTrack.artistName ?? currentExternalTrack.extractor)
				: null) ??
			(isRadioPlaying
				? radioNowPlaying?.artist && radioNowPlaying.artist !== radioTitle
					? radioNowPlaying.artist
					: "Live radio"
				: null) ??
			"Select a track");
	// An External Track is marked in the caption line an Album title uses.
	const nowPlayingCaption = currentExternalTrack
		? externalSourceName
			? `External – ${externalSourceName}`
			: "External"
		: (currentTrack?.albumTitle ?? null);
	// The bar shows a 56–64px cover; the full-screen views get a large one.
	// An External Track has one size, about 300px.
	const artworkUrl = currentTrack
		? getAlbumCoverUrl(currentTrack.albumId, 128)
		: currentExternalTrack
			? getExternalTrackArtworkUrl(currentExternalTrack)
			: (currentRadioStation?.faviconUrl ?? null);
	const largeArtworkUrl = currentTrack
		? getAlbumCoverUrl(currentTrack.albumId, 1024)
		: (currentRadioStation?.faviconUrl ?? null);
	const themeMode = useResolvedThemeMode(preferences.theme.mode);
	const coverAccent = useCoverAccent({
		cacheKey: currentTrack ? currentTrack.albumId : null,
		coverUrl: currentTrack ? artworkUrl : null,
		enabled: playbackPreferences.accentFromCover,
		mode: themeMode,
	});
	// Vintage Harbor takes only the cover's hue, on the root, so every accent
	// in the app follows it at the theme's own L and C; the bar then keeps the
	// theme's control colours instead of overriding them here.
	const hueAccent = preferences.theme.preset === "vintage-harbor";
	useRootAccentHue(coverAccent, hueAccent);
	const accent: Record<string, string> =
		hueAccent && coverAccent["--player-accent"]
			? {
					"--player-accent": "var(--player-control-primary)",
					"--player-accent-foreground":
						"var(--player-control-primary-foreground)",
					"--cover-tint": coverAccent["--cover-tint"],
				}
			: hueAccent
				? {}
				: coverAccent;

	const effectiveDuration =
		duration > 0
			? duration
			: currentTrack?.durationMs
				? currentTrack.durationMs / 1000
				: currentExternalTrack?.durationMs
					? currentExternalTrack.durationMs / 1000
					: 0;

	const qualityLabel = isRadioPlaying
		? formatRadioQualityLabel(currentRadioStation)
		: formatQualityLabel(currentTrack);
	const hasActiveSource =
		currentTrack !== null ||
		currentExternalTrack !== null ||
		currentRadioStation !== null;
	const outputAlert = playbackError ? null : outputDeviceIssue?.message;
	const qualityDetailRows = useMemo(
		() =>
			hasActiveSource
				? buildQualityDetailRows({
						telemetry:
							playbackTelemetry ??
							createFallbackPlaybackTelemetry(playbackSource, volume),
						processing: processingState,
						outputMode,
					})
				: [],
		[
			hasActiveSource,
			playbackTelemetry,
			playbackSource,
			volume,
			processingState,
			outputMode,
		],
	);
	// Catalog previews are not saved stations, so they cannot be favorited.
	const favoritableStationId =
		currentRadioStation && !currentRadioStation.id.startsWith("preview:")
			? currentRadioStation.id
			: null;
	const favoriteTarget = currentTrack
		? { kind: "track" as const, isFavorite: isCurrentTrackFavorite }
		: favoritableStationId
			? { kind: "station" as const, isFavorite: isCurrentStationFavorite }
			: null;
	const canToggleFavorite =
		favoriteTarget?.kind === "track"
			? Boolean(onToggleFavorite)
			: favoriteTarget?.kind === "station"
				? Boolean(onToggleStationFavorite)
				: false;
	const handleToggleFavorite = () => {
		if (currentTrack) {
			onToggleFavorite?.(currentTrack.id);
			return;
		}
		if (favoritableStationId) {
			onToggleStationFavorite?.(
				favoritableStationId,
				!isCurrentStationFavorite,
			);
		}
	};
	const sortedPlaylists = useMemo(
		() =>
			[...playlists].sort((a, b) => {
				if (a.isDefault !== b.isDefault) return a.isDefault ? -1 : 1;
				return a.name.localeCompare(b.name);
			}),
		[playlists],
	);

	const playlistSearchQuery = playlistQuery.trim().toLowerCase();

	const visiblePlaylists = (() => {
		if (playlistSearchQuery) {
			return sortedPlaylists.filter((playlist) =>
				playlist.name.toLowerCase().includes(playlistSearchQuery),
			);
		}
		const recentIds = readRecentPlaylistIds();
		const recent = recentIds
			.map((id) => sortedPlaylists.find((playlist) => playlist.id === id))
			.filter((playlist): playlist is Playlist => Boolean(playlist))
			.slice(0, RECENT_PLAYLIST_LIMIT);
		if (recent.length >= RECENT_PLAYLIST_LIMIT) return recent;
		const recentSet = new Set(recent.map((playlist) => playlist.id));
		for (const playlist of sortedPlaylists) {
			if (recent.length >= RECENT_PLAYLIST_LIMIT) break;
			if (!recentSet.has(playlist.id)) recent.push(playlist);
		}
		return recent.slice(0, RECENT_PLAYLIST_LIMIT);
	})();

	const closeActionsMenu = () => {
		if (playlistSubmenuCloseTimerRef.current !== null) {
			window.clearTimeout(playlistSubmenuCloseTimerRef.current);
			playlistSubmenuCloseTimerRef.current = null;
		}
		setPlaylistSubmenuOpen(false);
		setActionsOpen(false);
	};

	const openPlaylistSubmenu = () => {
		if (playlistSubmenuCloseTimerRef.current !== null) {
			window.clearTimeout(playlistSubmenuCloseTimerRef.current);
			playlistSubmenuCloseTimerRef.current = null;
		}
		setPlaylistSubmenuOpen(true);
		if (!playlistsLoaded && currentTrackId) void loadPlaylists(currentTrackId);
	};

	const closePlaylistSubmenu = () => {
		if (playlistSubmenuCloseTimerRef.current !== null) {
			window.clearTimeout(playlistSubmenuCloseTimerRef.current);
			playlistSubmenuCloseTimerRef.current = null;
		}
		setPlaylistSubmenuOpen(false);
		setPlaylistQuery("");
		setCreatePlaylistOpen(false);
		setNewPlaylistName("");
	};

	const schedulePlaylistSubmenuClose = () => {
		if (playlistSubmenuCloseTimerRef.current !== null) {
			window.clearTimeout(playlistSubmenuCloseTimerRef.current);
		}
		playlistSubmenuCloseTimerRef.current = window.setTimeout(() => {
			closePlaylistSubmenu();
		}, 200);
	};

	const openInfoModal = () => {
		closeActionsMenu();
		setInfoOpen(true);
	};

	const handleGoToAlbum = () => {
		if (!currentTrack) return;
		closeActionsMenu();
		void navigate({
			to: "/library/$albumId",
			params: { albumId: currentTrack.albumId },
		});
	};

	const artists = getTrackArtistCredits(currentTrack ?? { artists: [] });
	const handleGoToArtist = (artistId: string) => {
		closeActionsMenu();
		void goToArtistCreditsSearch(navigate, artistId);
	};

	const handlePlayNext = () => {
		if (!currentTrack) return;
		closeActionsMenu();
		void playNext(currentTrack.id);
	};

	const handleAddToQueue = () => {
		if (!currentTrack) return;
		closeActionsMenu();
		void addToQueue(currentTrack.id);
	};

	const sleepTimerLabel =
		sleepTimer.mode.kind === "after-track"
			? "After this track"
			: sleepTimer.mode.kind === "minutes"
				? `${formatRemaining(sleepTimer.remainingSeconds)} left`
				: "Off";

	const notifyPlaylistMutated = () => {
		onPlaylistMutated?.();
	};

	const refreshPlaylists = async () => {
		if (currentTrackId) {
			await loadPlaylists(currentTrackId);
		} else {
			setPlaylists((await listPlaylists()).items);
		}
		notifyPlaylistMutated();
	};

	const handleTogglePlaylist = async (playlistId: string) => {
		if (!currentTrack) return;
		if (memberPlaylistIds.has(playlistId)) {
			await removePlaylistTrack(playlistId, currentTrack.id);
			setMemberPlaylistIds((current) => {
				const next = new Set(current);
				next.delete(playlistId);
				return next;
			});
		} else {
			await addPlaylistTrack(playlistId, currentTrack.id);
			setMemberPlaylistIds((current) => new Set(current).add(playlistId));
			touchRecentPlaylist(playlistId);
		}
		const data = await listPlaylists();
		setPlaylists(data.items);
		notifyPlaylistMutated();
	};

	const handleCreatePlaylist = async () => {
		if (!currentTrack || !newPlaylistName.trim()) return;
		const playlist = await createPlaylist(newPlaylistName.trim());
		await addPlaylistTrack(playlist.id, currentTrack.id);
		touchRecentPlaylist(playlist.id);
		setNewPlaylistName("");
		setCreatePlaylistOpen(false);
		await refreshPlaylists();
	};

	return (
		<footer
			data-testid="player-bar"
			data-shell-surface
			style={
				{
					"--player-accent": accent["--player-accent"],
					"--player-accent-foreground": accent["--player-accent-foreground"],
					"--player-control-primary": accent["--player-control-primary"],
					"--player-control-primary-foreground":
						accent["--player-control-primary-foreground"],
					"--player-live-progress": accent["--player-live-progress"],
					"--player-control-shadow": accent["--player-control-shadow"],
				} as CSSProperties
			}
			className="relative h-21.5 border-player-border border-t bg-player-bar px-6 text-player-foreground"
		>
			{playbackError ? (
				<PlaybackErrorBanner error={playbackError} recovery={errorRecovery} />
			) : outputAlert ? (
				<p
					role="alert"
					className="absolute bottom-full left-1/2 mb-2 -translate-x-1/2 rounded-md border border-destructive/40 bg-popover px-3 py-2 text-destructive text-sm shadow-lg"
				>
					{outputAlert}
				</p>
			) : null}
			<div className="@container/player-bar flex h-full w-full min-w-0 items-center justify-between gap-6">
				<section
					aria-label="Now playing"
					className="@container/now-playing flex min-w-50 grow shrink-0 basis-0 items-center gap-3 justify-self-start sm:gap-4"
				>
					{currentTrack ? (
						<button
							type="button"
							data-player-control
							className="shrink-0 rounded-sm"
							aria-label="Open now playing"
							onClick={() => setNowPlayingOpen(true)}
						>
							<AlbumArt
								key={artworkUrl ?? "none"}
								coverUrl={artworkUrl}
								title={nowPlayingTitle}
								className="motion-safe:animate-player-cover size-14 rounded-sm border border-shell-subtle-border bg-player-artwork text-sm sm:size-16"
							/>
						</button>
					) : (
						<AlbumArt
							key={artworkUrl ?? "none"}
							coverUrl={artworkUrl}
							title={nowPlayingTitle}
							className="motion-safe:animate-player-cover size-14 shrink-0 rounded-sm border border-shell-subtle-border bg-player-artwork text-sm sm:size-16"
						/>
					)}
					<div className="min-w-0 flex-1 overflow-hidden">
						<div className="flex max-w-full min-w-0 items-center">
							<p
								key={nowPlayingTitle}
								className="motion-safe:animate-player-title min-w-0 truncate font-medium text-player-title text-sm sm:leading-6"
								title={nowPlayingTitle}
							>
								{nowPlayingTitle}
							</p>
							{/* An External Track is not in the library: no favorite, no actions. */}
							{(onToggleFavorite || onToggleStationFavorite) &&
							!currentExternalTrack ? (
								<button
									type="button"
									data-player-control
									className={cn(
										"ml-2 inline-flex size-6 relative after:absolute after:-inset-1 after:content-empty shrink-0 items-center justify-center rounded-sm text-player-foreground disabled:opacity-40",
										favoriteTarget?.isFavorite
											? "text-player-control-primary"
											: "hover:text-heading",
									)}
									aria-label={
										favoriteTarget?.isFavorite
											? "Remove from favorites"
											: "Add to favorites"
									}
									aria-pressed={favoriteTarget?.isFavorite ?? false}
									disabled={!canToggleFavorite}
									onClick={handleToggleFavorite}
								>
									<Heart
										className={cn(
											"size-3.5",
											favoriteTarget?.isFavorite && "fill-current",
										)}
									/>
								</button>
							) : null}
							{currentExternalTrack ? null : (
								<button
									ref={actionsButtonRef}
									type="button"
									data-player-control
									className={cn(
										"ml-1 inline-flex size-6 relative after:absolute after:-inset-1 after:content-empty shrink-0 items-center justify-center rounded-sm text-player-foreground hover:text-heading disabled:opacity-40",
										actionsOpen && "text-player-control-primary",
									)}
									aria-label="Track actions"
									aria-expanded={actionsOpen}
									disabled={!currentTrack}
									onClick={() => {
										setActionsOpen((open) => {
											const next = !open;
											if (next) updateMenuPosition();
											return next;
										});
									}}
								>
									<MoreVertical className="size-3.5" />
								</button>
							)}
						</div>
						<p
							className="truncate text-player-foreground text-xs sm:text-sm sm:leading-5"
							title={nowPlayingSubtitle}
							role={isReconnecting ? "status" : undefined}
							aria-live={isReconnecting ? "polite" : undefined}
						>
							{nowPlayingSubtitle}
						</p>
						{nowPlayingCaption ? (
							<p
								className="hidden truncate text-caption text-xs sm:block sm:leading-5"
								title={nowPlayingCaption}
							>
								{nowPlayingCaption}
							</p>
						) : null}
						{actionsOpen && currentTrack && menuPosition ? (
							<Portal>
								<div
									id="player-track-actions-menu"
									className="fixed top-(--menu-top) left-(--menu-left) z-50 min-w-44 -translate-y-full rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg"
									style={
										{
											"--menu-top": `${menuPosition.top}px`,
											"--menu-left": `${menuPosition.left}px`,
										} as CSSProperties
									}
									role="menu"
								>
									{hidePlaylists ? null : (
										<AddToPlaylistMenuItem
											open={playlistSubmenuOpen}
											onOpen={openPlaylistSubmenu}
											onClose={schedulePlaylistSubmenuClose}
											query={playlistQuery}
											onQueryChange={setPlaylistQuery}
											createOpen={createPlaylistOpen}
											onCreateOpen={() => setCreatePlaylistOpen(true)}
											newPlaylistName={newPlaylistName}
											onNewPlaylistNameChange={setNewPlaylistName}
											playlists={visiblePlaylists}
											memberPlaylistIds={memberPlaylistIds}
											isSearching={Boolean(playlistSearchQuery)}
											onToggle={(playlistId) =>
												void handleTogglePlaylist(playlistId)
											}
											onCreate={() => void handleCreatePlaylist()}
										/>
									)}
									{isInRoom ? null : (
										<>
											<MenuButton onClick={handlePlayNext}>
												<ListEnd className="size-3.5" />
												Play next
											</MenuButton>
											<MenuButton onClick={handleAddToQueue}>
												<ListPlus className="size-3.5" />
												Add to queue
											</MenuButton>
										</>
									)}
									<MenuButton onClick={handleGoToAlbum}>Go to album</MenuButton>
									{artists.length > 1 ? (
										<MenuSection icon={null} label="Go to artist" value="">
											{artists.map((artist) => (
												<MenuChoice
													key={artist.id}
													label={artist.name}
													onClick={() => handleGoToArtist(artist.id)}
												/>
											))}
										</MenuSection>
									) : (
										<MenuButton
											disabled={!artists.length}
											onClick={() => handleGoToArtist(artists[0].id)}
										>
											Go to artist
										</MenuButton>
									)}
									{isInRoom ? null : (
										<>
											<MenuSection
												icon={<Gauge className="size-3.5" />}
												label="Speed"
												value={`${playbackRate}×`}
											>
												{PLAYBACK_RATES.map((rate) => (
													<MenuChoice
														key={rate}
														checked={playbackRate === rate}
														label={`${rate}×`}
														onClick={() => setPlaybackRate(rate)}
													/>
												))}
											</MenuSection>
											<MenuSection
												icon={<Repeat1 className="size-3.5" />}
												label="A-B repeat"
												value={
													abRepeat.isActive
														? `${formatMenuTime(abRepeat.a)}–${formatMenuTime(abRepeat.b)}`
														: abRepeat.a !== null
															? `A ${formatMenuTime(abRepeat.a)}`
															: "Off"
												}
											>
												<MenuChoice
													label="Set A"
													onClick={() => abRepeat.setPoint("a")}
												/>
												<MenuChoice
													label="Set B"
													disabled={abRepeat.a === null}
													onClick={() => abRepeat.setPoint("b")}
												/>
												<MenuChoice
													label="Clear"
													disabled={abRepeat.a === null && abRepeat.b === null}
													onClick={abRepeat.clear}
												/>
											</MenuSection>
											<MenuSection
												icon={<MoonStar className="size-3.5" />}
												label="Sleep timer"
												value={sleepTimerLabel}
											>
												<MenuChoice
													checked={sleepTimer.mode.kind === "off"}
													label="Off"
													onClick={() =>
														sleepTimer.setSleepTimer({ kind: "off" })
													}
												/>
												<MenuChoice
													checked={sleepTimer.mode.kind === "after-track"}
													label="After track"
													onClick={() =>
														sleepTimer.setSleepTimer({ kind: "after-track" })
													}
												/>
												{SLEEP_TIMER_MINUTES.map((minutes) => (
													<MenuChoice
														key={minutes}
														checked={
															sleepTimer.mode.kind === "minutes" &&
															sleepTimer.mode.minutes === minutes
														}
														label={`${minutes} min`}
														onClick={() =>
															sleepTimer.setSleepTimer({
																kind: "minutes",
																minutes,
															})
														}
													/>
												))}
											</MenuSection>
										</>
									)}
									<MenuButton disabled>
										<Download className="size-3.5" />
										Download
									</MenuButton>
									<MenuButton onClick={openInfoModal}>
										<Info className="size-3.5" />
										Details
									</MenuButton>
								</div>
							</Portal>
						) : null}
					</div>
					{playbackPreferences.showUpNext &&
					preferences.layout.collapsed[queuePanelSide] ? (
						<UpNextPeek />
					) : null}
				</section>

				<PlaybackControls
					isRadioPlaying={isRadioPlaying}
					isPlaying={isPlaying}
					hasPlayableSource={hasPlayableSource}
					hasCurrentTrack={Boolean(currentTrack)}
					canSeek={canSeek}
					showQueueControls={!currentExternalTrack}
					currentTime={currentTime}
					effectiveDuration={effectiveDuration}
					bufferedEnd={bufferedEnd}
					waveform={waveform}
					showHoverTimestamp={playbackPreferences.hoverTimestamp}
					keyboardStepSeconds={playbackPreferences.seekStepSeconds}
					keyboardLargeStepSeconds={playbackPreferences.seekStepLargeSeconds}
					shuffleEnabled={shuffleEnabled}
					repeatMode={repeatMode}
					onTogglePlay={togglePlay}
					onToggleShuffle={toggleShuffle}
					onCycleRepeatMode={cycleRepeatMode}
					onPrevious={navigatePrevious}
					onNext={navigateNext}
					onSeek={seek}
				/>

				<VolumeAndQueueControls
					qualityLabel={qualityLabel}
					isLossless={isLosslessFormat(currentTrack?.format)}
					qualityDetailRows={qualityDetailRows}
					isGapless={isGapless === true && hasActiveSource}
					volume={volume}
					onToggleMute={toggleMute}
					signalControl={
						hasActiveSource && outputMode ? (
							<PlaybackSignal
								qualityLabel={qualityLabel}
								qualityIcon={
									<QualityIconFor
										isLossless={isLosslessFormat(currentTrack?.format)}
									/>
								}
								outputMode={outputMode}
								outputControls={{
									selectNormalOutput: fallbackToSystemOutput,
									selectExclusiveOutput,
									enableAdaptiveSystemRate,
								}}
							/>
						) : undefined
					}
					isQueueOpen={!preferences.layout.collapsed[queuePanelSide]}
					upcomingCount={upcomingCount}
					hasQueueItems={queue.length > 0}
					onToggleQueue={toggleQueue}
					onOpenLyrics={currentTrack ? () => setLyricsOpen(true) : undefined}
					onVolumeChange={setVolume}
				/>
			</div>
			<NowPlayingAnnouncer />
			{lyricsOpen && currentTrack ? (
				<LyricsOverlay
					track={currentTrack}
					coverUrl={largeArtworkUrl}
					accent={accent}
					windowControls={windowControls}
					loadLyrics={getTrackLyrics}
					onClose={() => setLyricsOpen(false)}
				/>
			) : null}
			{helpOpen ? (
				<ShortcutHelpOverlay
					options={{
						seekStepSeconds: playbackPreferences.seekStepSeconds,
						seekStepLargeSeconds: playbackPreferences.seekStepLargeSeconds,
					}}
					onClose={() => setHelpOpen(false)}
				/>
			) : null}
			{infoOpen && currentTrack ? (
				<TrackInfoDialog
					track={currentTrack}
					onClose={() => setInfoOpen(false)}
				/>
			) : null}
			{nowPlayingOpen && currentTrack ? (
				<NowPlayingView
					track={currentTrack}
					coverUrl={largeArtworkUrl}
					accent={accent}
					onOpenLyrics={() => {
						setNowPlayingOpen(false);
						setLyricsOpen(true);
					}}
					onClose={() => setNowPlayingOpen(false)}
				/>
			) : null}
		</footer>
	);
}

function Portal({ children }: { children: ReactNode }) {
	if (typeof document === "undefined") return null;
	return createPortal(children, document.body);
}

function AddToPlaylistMenuItem({
	open,
	onOpen,
	onClose,
	query,
	onQueryChange,
	createOpen,
	onCreateOpen,
	newPlaylistName,
	onNewPlaylistNameChange,
	playlists,
	memberPlaylistIds,
	isSearching,
	onToggle,
	onCreate,
}: {
	open: boolean;
	onOpen: () => void;
	onClose: () => void;
	query: string;
	onQueryChange: (value: string) => void;
	createOpen: boolean;
	onCreateOpen: () => void;
	newPlaylistName: string;
	onNewPlaylistNameChange: (value: string) => void;
	playlists: {
		id: string;
		name: string;
		isDefault: boolean;
		trackCount: number;
	}[];
	memberPlaylistIds: Set<string>;
	isSearching: boolean;
	onToggle: (playlistId: string) => void;
	onCreate: () => void;
}) {
	return (
		<div
			className="relative"
			onMouseEnter={onOpen}
			onMouseLeave={onClose}
			role="none"
		>
			<div
				className={cn(
					"flex w-full items-center justify-between gap-2 rounded-sm px-2 py-2 text-left text-xs",
					open && "bg-muted text-heading",
				)}
				role="menuitem"
				aria-haspopup="menu"
				aria-expanded={open}
				tabIndex={0}
				onFocus={onOpen}
				onBlur={onClose}
			>
				<span className="flex items-center gap-2">Add to playlist</span>
				<ChevronRight className="size-3.5 text-caption" />
			</div>
			{open ? (
				<div
					className="absolute top-0 left-full z-50 ml-1 w-56 rounded-md border border-border bg-popover p-2 text-popover-foreground shadow-lg"
					role="menu"
					tabIndex={-1}
					aria-label="Add to playlist"
					onMouseEnter={onOpen}
					onMouseLeave={onClose}
				>
					<input
						type="search"
						placeholder="Search playlists"
						value={query}
						onChange={(event) => onQueryChange(event.target.value)}
						className="mb-2 h-8 w-full rounded-sm border border-border bg-background px-3 text-xs outline-none focus:ring-2 focus:ring-primary/40"
						onClick={(event) => event.stopPropagation()}
					/>
					{createOpen ? (
						<div className="mb-2 flex gap-2">
							<label className="sr-only" htmlFor="player-new-playlist-name">
								New playlist name
							</label>
							<input
								id="player-new-playlist-name"
								value={newPlaylistName}
								onChange={(event) =>
									onNewPlaylistNameChange(event.target.value)
								}
								className="h-8 min-w-0 flex-1 rounded-sm border border-border bg-background px-3 text-xs outline-none focus:ring-2 focus:ring-primary/40"
								placeholder="Playlist name"
							/>
							<button
								type="button"
								className="inline-flex h-8 items-center rounded-sm bg-primary px-3 font-medium text-primary-foreground text-xs disabled:opacity-50"
								disabled={!newPlaylistName.trim()}
								onClick={onCreate}
							>
								Create
							</button>
						</div>
					) : (
						<button
							type="button"
							className="mb-2 flex w-full items-center gap-2 rounded-sm px-2 py-2 text-left text-xs hover:bg-muted"
							role="menuitem"
							onClick={onCreateOpen}
						>
							<span className="inline-flex size-5 items-center justify-center rounded-full border border-border">
								<Plus className="size-3" />
							</span>
							Create new playlist
						</button>
					)}
					{!isSearching && playlists.length > 0 ? (
						<p className="mb-1 px-2 font-medium text-xs text-caption uppercase tracking-wide">
							Recent
						</p>
					) : null}
					<div className="max-h-40 overflow-auto">
						{playlists.length === 0 ? (
							<p className="px-2 py-2 text-caption text-xs">
								{isSearching ? "No playlists found" : "No playlists yet"}
							</p>
						) : (
							playlists.map((playlist) => {
								const isMember = memberPlaylistIds.has(playlist.id);
								return (
									<button
										key={playlist.id}
										type="button"
										className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-2 text-left text-xs hover:bg-muted"
										role="menuitem"
										aria-label={
											isMember
												? `Remove from ${playlist.name}`
												: `Add to ${playlist.name}`
										}
										onClick={() => onToggle(playlist.id)}
									>
										<span className="min-w-0 truncate">{playlist.name}</span>
										{isMember ? (
											<Check className="size-3.5 shrink-0 text-heading" />
										) : null}
									</button>
								);
							})
						)}
					</div>
				</div>
			) : null}
		</div>
	);
}

function MenuButton({
	children,
	disabled = false,
	onClick,
}: {
	children: ReactNode;
	disabled?: boolean;
	onClick?: () => void;
}) {
	return (
		<button
			type="button"
			className="flex w-full items-center gap-2 rounded-sm px-2 py-2 text-left text-xs hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
			role="menuitem"
			disabled={disabled}
			onClick={onClick}
		>
			{children}
		</button>
	);
}

function TrackInfoDialog({
	track,
	onClose,
}: {
	track: Track;
	onClose: () => void;
}) {
	const rows = buildTrackDetailRows(track);
	const dialogRef = useRef<HTMLDivElement>(null);
	useFocusTrap(dialogRef);

	return (
		<Portal>
			<div className="fixed inset-0 z-50 flex items-center justify-center bg-background/70 p-4">
				<div
					ref={dialogRef}
					role="dialog"
					aria-modal="true"
					aria-label={track.title}
					tabIndex={-1}
					className="max-h-dialog-height w-full max-w-2xl overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-xl"
				>
					<div className="flex items-center justify-between gap-3 border-border border-b p-4">
						<h2 className="truncate font-semibold text-heading text-xl">
							{track.title}
						</h2>
						<button
							type="button"
							className="inline-flex size-8 items-center justify-center rounded-full hover:bg-muted"
							aria-label="Close"
							onClick={onClose}
						>
							<X className="size-4" />
						</button>
					</div>
					<div className="max-h-dialog-body-height overflow-auto p-4">
						{rows.map(([label, value]) => (
							<div
								key={label}
								className="grid grid-cols-label gap-3 border-border border-b py-2 text-sm"
							>
								<span className="text-caption">{label}</span>
								<span className="min-w-0 break-words font-medium text-foreground">
									{value}
								</span>
							</div>
						))}
					</div>
				</div>
			</div>
		</Portal>
	);
}

function formatMenuTime(seconds: number | null): string {
	if (seconds === null || !Number.isFinite(seconds)) return "–";
	const minutes = Math.floor(seconds / 60);
	const remaining = Math.floor(seconds % 60);
	return `${minutes}:${remaining.toString().padStart(2, "0")}`;
}

function formatRemaining(seconds: number | null): string {
	if (seconds === null) return "";
	const minutes = Math.ceil(seconds / 60);
	return minutes <= 1 ? "<1 min" : `${minutes} min`;
}

/** Labelled group inside the actions menu holding a row of choices. */
function MenuSection({
	icon,
	label,
	value,
	children,
}: {
	icon: ReactNode;
	label: string;
	value: string;
	children: ReactNode;
}) {
	return (
		<fieldset
			aria-label={label}
			className="m-0 mt-1 min-w-0 border-0 border-border border-t p-0 pt-1"
		>
			<div className="flex items-center justify-between gap-2 px-2 py-1 text-xs text-caption uppercase tracking-wide">
				<span className="flex items-center gap-2">
					{icon}
					{label}
				</span>
				<span className="normal-case tabular-nums">{value}</span>
			</div>
			<div className="flex flex-wrap gap-1 px-1 pb-1">{children}</div>
		</fieldset>
	);
}

function MenuChoice({
	label,
	checked,
	disabled = false,
	onClick,
}: {
	label: string;
	checked?: boolean;
	disabled?: boolean;
	onClick: () => void;
}) {
	const className = cn(
		"rounded-sm border border-border px-2 py-0.5 text-xs hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50",
		checked && "border-primary bg-muted text-heading",
	);
	if (checked === undefined) {
		return (
			<button
				type="button"
				role="menuitem"
				disabled={disabled}
				className={className}
				onClick={onClick}
			>
				{label}
			</button>
		);
	}
	return (
		<button
			type="button"
			role="menuitemradio"
			aria-checked={checked}
			disabled={disabled}
			className={className}
			onClick={onClick}
		>
			{label}
		</button>
	);
}
