import type {
	CoverSize,
	Playlist,
	PlaylistDetail,
	PlaylistList,
	QueueItem,
	QueueItemSource,
	RadioNowPlaying,
	RadioSearchResult,
	RadioStation,
	Track,
} from "@repo/api-client";
import {
	createContext,
	type ReactNode,
	useCallback,
	useContext,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import type {
	PlaybackEngine,
	PlaybackError,
	PlaybackErrorCause,
	PlaybackSessionState,
	PlaybackSource,
	RepeatMode,
} from "./PlaybackEngine";
import type {
	EqualizerPreset,
	OutputDevice,
	OutputDeviceIssue,
	OutputMode,
	ProcessingProfile,
	ProcessingState,
	ReplayGainMode,
} from "./processing";
import type { PlaybackTelemetry } from "./telemetry";
import { type AbRepeat, useAbRepeat } from "./use-ab-repeat";
import { type SleepTimer, useSleepTimer } from "./use-sleep-timer";
import {
	type PlaybackQueueApi,
	useSynchronizedQueue,
} from "./use-synchronized-queue";
import type { TrackWaveformLoader } from "./use-track-waveform";

export type { RepeatMode } from "./PlaybackEngine";
export type { PlaybackQueueApi } from "./use-synchronized-queue";

export type PlaybackAssetApi = {
	getStreamUrl: (trackId: string) => string;
	/**
	 * Optional HEAD probe used to explain a failed Track: a 404 means the
	 * file is missing on the server, a rejection means the server is
	 * unreachable. Hosts without it get an "unknown" cause.
	 */
	headTrackStream?: (trackId: string) => Promise<{ status: number }>;
	getAlbumCoverUrl: (albumId: string, size?: CoverSize) => string;
	getTrack?: (trackId: string) => Promise<Track>;
	/** Lyrics text stored for a Track; optional for hosts without the endpoint. */
	getTrackLyrics?: (trackId: string) => Promise<{ lyrics: string }>;
	/** Waveform peaks, or a pending marker while the server generates them. */
	getTrackWaveform?: TrackWaveformLoader;
};

export type PlaylistLibraryApi = {
	listPlaylists: () => Promise<PlaylistList>;
	getPlaylist: (playlistId: string) => Promise<PlaylistDetail>;
	createPlaylist: (name: string) => Promise<Playlist>;
	addPlaylistTrack: (
		playlistId: string,
		trackId: string,
	) => Promise<PlaylistDetail>;
	removePlaylistTrack: (
		playlistId: string,
		trackId: string,
	) => Promise<PlaylistDetail>;
};

export type RadioPlaybackApi = {
	getRadioStationStreamUrl: (stationId: string) => string;
	getRadioCatalogPreviewStreamUrl: (stationUuid: string) => string;
	getRadioNowPlaying: (stationId: string) => Promise<RadioNowPlaying>;
};

export type PlaybackApi = PlaybackQueueApi &
	PlaybackAssetApi &
	PlaylistLibraryApi &
	RadioPlaybackApi;

type PlaybackContextValue = {
	queue: QueueItem[];
	playbackSource: PlaybackSource | null;
	outputMode: OutputMode | null;
	availableOutputDevices: OutputDevice[];
	selectedOutputDevice: OutputDevice | null;
	outputDeviceIssue: OutputDeviceIssue | null;
	currentTrack: Track | null;
	currentRadioStation: RadioStation | null;
	radioNowPlaying: RadioNowPlaying | null;
	isPlaying: boolean;
	isReconnecting: boolean;
	currentTime: number;
	duration: number;
	bufferedEnd: number | null;
	volume: number;
	playbackRate: number;
	/** Whether the engine plays consecutive tracks without a gap; null when unknown. */
	isGapless: boolean | null;
	sleepTimer: SleepTimer;
	abRepeat: AbRepeat;
	shuffleEnabled: boolean;
	repeatMode: RepeatMode;
	playbackError: PlaybackError | null;
	/** Present while a Track failed and the user can retry or skip it. */
	errorRecovery: PlaybackErrorRecovery | null;
	processingState: ProcessingState | null;
	playbackTelemetry: PlaybackTelemetry | null;
	queueConflict: string | null;
	playTrack: (
		trackId: string,
		queueTrackIds?: string[],
		source?: QueueItemSource,
	) => Promise<void>;
	playRadioStation: (station: RadioStation) => Promise<void>;
	playRadioCatalogPreview: (result: RadioSearchResult) => Promise<void>;
	queueTracks: (trackIds: string[], source?: QueueItemSource) => Promise<void>;
	playQueueIndex: (index: number) => Promise<void>;
	playNext: (trackId: string) => Promise<void>;
	navigatePrevious: () => void;
	navigateNext: () => void;
	togglePlay: () => void;
	toggleShuffle: () => void;
	cycleRepeatMode: () => void;
	seek: (seconds: number) => void;
	setVolume: (value: number) => void;
	setPlaybackRate: (rate: number) => void;
	setTransitionFade: (milliseconds: number) => void;
	/** Appends a Track to the end of the Queue. */
	addToQueue: (trackId: string) => Promise<void>;
	setProcessingProfile: (profile: ProcessingProfile) => void;
	setReplayGainMode: (mode: ReplayGainMode) => void;
	setEqualizerPreset: (preset: Exclude<EqualizerPreset, "custom">) => void;
	setEqualizerGain: (index: number, value: number) => void;
	refreshOutputDevices: () => void;
	selectDirectAlsaOutput: (deviceId: string) => void;
	selectExclusiveOutput: () => void;
	fallbackToSystemOutput: () => void;
	enableAdaptiveSystemRate: () => void;
	removeFromQueue: (itemId: string) => Promise<void>;
	reorderQueue: (itemIds: string[]) => Promise<void>;
	clearQueue: () => Promise<void>;
	refreshQueue: () => Promise<void>;
	stopPlayback: () => void;
	getAlbumCoverUrl: (albumId: string, size?: CoverSize) => string;
	getTrack: (trackId: string) => Promise<Track | null>;
	/** Resolves null when the host exposes no lyrics endpoint. */
	getTrackLyrics: (trackId: string) => Promise<{ lyrics: string } | null>;
	/** Null when the host exposes no waveform endpoint. */
	getTrackWaveform: TrackWaveformLoader | null;
};

export type PlaybackErrorRecovery = {
	cause: PlaybackErrorCause | null;
	/** Seconds until the Track is skipped automatically; null once stopped. */
	countdownSeconds: number | null;
	canSkip: boolean;
	retry: () => void;
	skip: () => void;
	cancelCountdown: () => void;
};

// Past this point Previous restarts the Track instead of going back one.
const PREVIOUS_RESTARTS_AFTER_SECONDS = 30;

const PlaybackContext = createContext<PlaybackContextValue | null>(null);
const PlaylistLibraryContext = createContext<PlaylistLibraryApi | null>(null);

export function PlaybackProvider({
	children,
	api,
	engine,
	autoSkipOnErrorSeconds = 5,
	shouldCoordinateQueue = true,
}: {
	children: ReactNode;
	api: PlaybackApi;
	engine: PlaybackEngine;
	/** Countdown before a failed Track is skipped; 0 waits for the user. */
	autoSkipOnErrorSeconds?: number;
	/** Only the main desktop window coordinates the native queue. */
	shouldCoordinateQueue?: boolean;
}) {
	const apiRef = useRef(api);
	const {
		queue,
		queueRef,
		queueConflict,
		refreshQueue,
		appendQueueItem,
		replaceQueue,
		removeFromQueue,
		clearQueue: clearQueueAfter,
		reorderQueue,
	} = useSynchronizedQueue(api);
	const subscribeToEngine = useCallback(
		(onChange: () => void) => engine.subscribe(onChange),
		[engine],
	);
	const session = useSyncExternalStore(subscribeToEngine, () =>
		engine.getState(),
	);
	const currentQueueItemIdRef = useRef<string | null>(null);
	const currentQueueIndexRef = useRef<number | null>(null);
	const repeatCancellationRequestedModeRef = useRef<RepeatMode | null>(null);
	const [radioNowPlaying, setRadioNowPlaying] =
		useState<RadioNowPlaying | null>(null);
	useLayoutEffect(() => {
		apiRef.current = api;
	});
	// The queue item a play request started; the session reports its own once
	// the engine confirms it.
	const [requestedQueueItemId, setRequestedQueueItemId] = useState<
		string | null
	>(null);

	const currentTrack =
		session.source?.type === "track" ? session.source.track : null;
	const currentRadioStation = useMemo(
		() => getCurrentRadioStation(session.source),
		[session.source],
	);
	const radioNowPlayingStationId =
		session.source?.type === "radio-station" ? session.source.station.id : null;

	const refreshRadioNowPlaying = useCallback(async () => {
		if (!radioNowPlayingStationId) return;
		try {
			const data = await apiRef.current.getRadioNowPlaying(
				radioNowPlayingStationId,
			);
			setRadioNowPlaying(data);
		} catch (error) {
			console.warn("Failed to refresh radio now playing", {
				stationId: radioNowPlayingStationId,
				error,
			});
		}
	}, [radioNowPlayingStationId]);

	useEffect(() => {
		if (!radioNowPlayingStationId) return undefined;
		void refreshRadioNowPlaying();
		const intervalId = window.setInterval(
			() => void refreshRadioNowPlaying(),
			30000,
		);
		return () => window.clearInterval(intervalId);
	}, [radioNowPlayingStationId, refreshRadioNowPlaying]);

	const playQueueItemInternal = useCallback(
		async (item: QueueItem, queueOverride?: QueueItem[]) => {
			const activeQueue = queueOverride ?? queueRef.current;
			const itemIndex = activeQueue.findIndex((entry) => entry.id === item.id);
			if (itemIndex < 0) return;
			currentQueueItemIdRef.current = item.id;
			currentQueueIndexRef.current = itemIndex;
			setRequestedQueueItemId(item.id);
			setRadioNowPlaying(null);
			try {
				await engine.syncQueueContext?.(
					queuePlaybackSources(activeQueue, apiRef.current),
					itemIndex,
				);
				await engine.play({
					type: "track",
					track: item.track,
					playbackUrl: apiRef.current.getStreamUrl(item.trackId),
					queueItemId: item.id,
					artworkUrl: apiRef.current.getAlbumCoverUrl(item.track.albumId, 512),
				});
			} catch {
				// PlaybackEngine exposes the error through observable session state.
			}
		},
		[queueRef, engine],
	);

	useEffect(() => {
		if (engine.syncQueueContext || !engine.subscribeNavigation)
			return undefined;
		return engine.subscribeNavigation((direction) => {
			const currentIndex = queueRef.current.findIndex(
				(item) => item.id === currentQueueItemIdRef.current,
			);
			if (currentIndex < 0) return;
			const offset = direction === "previous" ? -1 : 1;
			const item = queueRef.current[currentIndex + offset];
			if (item) void playQueueItemInternal(item);
		});
	}, [queueRef, engine, playQueueItemInternal]);

	useEffect(() => {
		if (!shouldCoordinateQueue || !engine.syncQueueContext) return;
		const currentIndex = queue.findIndex(
			(item) => item.id === currentQueueItemIdRef.current,
		);
		void engine
			.syncQueueContext(
				queuePlaybackSources(queue, apiRef.current),
				currentIndex >= 0 ? currentIndex : null,
			)
			.catch((error) => {
				console.warn("Failed to sync native playback Queue context", { error });
			});
	}, [engine, queue, shouldCoordinateQueue]);

	const playTrackInternal = useCallback(
		async (trackId: string, queueOverride?: QueueItem[]) => {
			const activeQueue = queueOverride ?? queueRef.current;
			const item = activeQueue.find(
				(entry) => entry.track.id === trackId || entry.trackId === trackId,
			);
			if (item) await playQueueItemInternal(item, activeQueue);
		},
		[queueRef, playQueueItemInternal],
	);

	useEffect(() => {
		if (session.source?.type !== "track") return;
		if (session.source.queueItemId) {
			currentQueueItemIdRef.current = session.source.queueItemId;
		}
		const itemIndex = queue.findIndex(
			(item) => item.id === currentQueueItemIdRef.current,
		);
		if (itemIndex < 0) return;
		currentQueueIndexRef.current = itemIndex;
	}, [queue, session.source]);

	useEffect(() => {
		if (!shouldCoordinateQueue) return;
		if (session.repeatMode === "off") {
			repeatCancellationRequestedModeRef.current = null;
			return;
		}
		if (queue.length > 0 || session.source?.type !== "track") {
			repeatCancellationRequestedModeRef.current = null;
			return;
		}
		if (repeatCancellationRequestedModeRef.current === session.repeatMode)
			return;
		repeatCancellationRequestedModeRef.current = session.repeatMode;
		engine.cycleRepeatMode();
	}, [
		engine,
		queue.length,
		session.repeatMode,
		session.source,
		shouldCoordinateQueue,
	]);

	const advanceToNextQueueItem = useCallback(() => {
		const index = queueRef.current.findIndex(
			(item) => item.id === currentQueueItemIdRef.current,
		);
		const nextIndex =
			index >= 0 ? index + 1 : (currentQueueIndexRef.current ?? 0);
		const next = queueRef.current[nextIndex];
		if (next) {
			void playQueueItemInternal(next);
			return;
		}
		engine.stop();
	}, [queueRef, engine, playQueueItemInternal]);

	useEffect(() => {
		if (engine.syncQueueContext) return;
		if (session.status !== "ended" || session.source?.type !== "track") return;
		if (session.stopAfterCurrent) {
			// Sleep timer "after this track": stay on the ended Track and disarm.
			engine.stop();
			engine.setStopAfterCurrent?.(false);
			return;
		}
		advanceToNextQueueItem();
	}, [advanceToNextQueueItem, engine, session]);

	const sleepTimer = useSleepTimer(engine, session);
	const abRepeat = useAbRepeat(engine, session);
	// Stable identity so the waveform hook does not refetch on every render.
	const getTrackWaveform = useMemo<TrackWaveformLoader | null>(
		() =>
			api.getTrackWaveform
				? (trackId) => apiRef.current.getTrackWaveform?.(trackId) as never
				: null,
		[api.getTrackWaveform],
	);

	const getTrack = useCallback(
		(trackId: string) =>
			apiRef.current.getTrack?.(trackId) ?? Promise.resolve(null),
		[],
	);

	const getTrackLyrics = useCallback(
		(trackId: string) =>
			apiRef.current.getTrackLyrics?.(trackId) ?? Promise.resolve(null),
		[],
	);

	// A Track can stop being playable while it sits in the Queue: ADR 0010 lets
	// a deletion elsewhere leave the playing source in place, so its next play
	// fails. Explain the failure in the bar, let the user retry or skip, and
	// skip on their behalf after a countdown so the Player never sits stuck.
	const failedSource = session.status === "error" ? session.source : null;
	const failedTrackId =
		failedSource?.type === "track" ? failedSource.track.id : null;
	const [errorCause, setErrorCause] = useState<PlaybackErrorCause | null>(null);
	const [countdownSeconds, setCountdownSeconds] = useState<number | null>(null);
	// A newly failed Track restarts the diagnosis and the skip countdown.
	const [diagnosedTrackId, setDiagnosedTrackId] = useState(failedTrackId);
	if (failedTrackId !== diagnosedTrackId) {
		setDiagnosedTrackId(failedTrackId);
		setErrorCause(failedTrackId && !api.headTrackStream ? "unknown" : null);
		setCountdownSeconds(
			failedTrackId && shouldCoordinateQueue && autoSkipOnErrorSeconds > 0
				? autoSkipOnErrorSeconds
				: null,
		);
	}
	const skipFailedTrack = useCallback(() => {
		setCountdownSeconds(null);
		if (engine.syncQueueContext) {
			engine.next();
			return;
		}
		advanceToNextQueueItem();
	}, [advanceToNextQueueItem, engine]);
	const retryFailedTrack = useCallback(() => {
		setCountdownSeconds(null);
		if (!failedSource) return;
		void engine.play(failedSource).catch(() => undefined);
	}, [engine, failedSource]);

	useEffect(() => {
		const probe = apiRef.current.headTrackStream;
		if (!failedTrackId || !probe) return undefined;
		let cancelled = false;
		probe(failedTrackId)
			.then(({ status }) => {
				if (cancelled) return;
				setErrorCause(status === 404 ? "file-missing" : "unknown");
			})
			.catch(() => {
				if (!cancelled) setErrorCause("network");
			});
		return () => {
			cancelled = true;
		};
	}, [failedTrackId]);

	useEffect(() => {
		if (countdownSeconds === null || !failedTrackId) return undefined;
		const timer = window.setTimeout(() => {
			if (countdownSeconds <= 1) skipFailedTrack();
			else setCountdownSeconds(countdownSeconds - 1);
		}, 1000);
		return () => window.clearTimeout(timer);
	}, [countdownSeconds, failedTrackId, skipFailedTrack]);

	const currentQueueItemId =
		(session.source?.type === "track" ? session.source.queueItemId : null) ??
		requestedQueueItemId;
	const currentQueueIndex = queue.findIndex(
		(item) => item.id === currentQueueItemId,
	);
	const hasNextQueueItem =
		currentQueueIndex >= 0 && currentQueueIndex + 1 < queue.length;

	const errorRecovery = useMemo<PlaybackErrorRecovery | null>(
		() =>
			failedTrackId
				? {
						cause: errorCause,
						countdownSeconds,
						canSkip: hasNextQueueItem,
						retry: retryFailedTrack,
						skip: skipFailedTrack,
						cancelCountdown: () => setCountdownSeconds(null),
					}
				: null,
		[
			failedTrackId,
			errorCause,
			countdownSeconds,
			hasNextQueueItem,
			retryFailedTrack,
			skipFailedTrack,
		],
	);

	const playTrack = useCallback(
		async (
			trackId: string,
			queueTrackIds?: string[],
			source?: QueueItemSource,
		) => {
			let nextQueue = queueRef.current;
			if (queueTrackIds) {
				const data = await replaceQueue(queueTrackIds, {
					retryOnConflict: true,
					source,
				});
				if (!data) return;
				nextQueue = data.items;
			}
			if (!nextQueue.some((item) => item.track.id === trackId)) {
				const data = await appendQueueItem(trackId, source);
				nextQueue = data.items;
			}
			await playTrackInternal(trackId, nextQueue);
		},
		[queueRef, appendQueueItem, playTrackInternal, replaceQueue],
	);

	const playRadioStation = useCallback(
		async (station: RadioStation) => {
			setRadioNowPlaying(station.lastNowPlaying ?? null);
			try {
				await engine.play({
					type: "radio-station",
					station,
					playbackUrl: apiRef.current.getRadioStationStreamUrl(station.id),
					sourceUrl: station.streamUrl,
				});
			} catch (error) {
				console.warn("Failed to start radio station playback", {
					stationId: station.id,
					error,
				});
			}
		},
		[engine],
	);

	const playRadioCatalogPreview = useCallback(
		async (result: RadioSearchResult) => {
			setRadioNowPlaying(null);
			try {
				await engine.play({
					type: "catalog-preview",
					result,
					playbackUrl: apiRef.current.getRadioCatalogPreviewStreamUrl(
						result.stationUuid,
					),
					sourceUrl: result.streamUrl,
				});
			} catch (error) {
				console.warn("Failed to start radio catalog preview", {
					stationUuid: result.stationUuid,
					error,
				});
				throw error;
			}
		},
		[engine],
	);

	const queueTracks = useCallback(
		async (trackIds: string[], source: QueueItemSource = { kind: "user" }) => {
			for (const trackId of trackIds) {
				await appendQueueItem(trackId, source);
			}
		},
		[appendQueueItem],
	);

	const playQueueIndex = useCallback(
		async (index: number) => {
			const item = queueRef.current[index];
			if (item) await playQueueItemInternal(item);
		},
		[queueRef, playQueueItemInternal],
	);

	const playNext = useCallback(
		async (trackId: string) => {
			const data = await appendQueueItem(trackId, { kind: "user" });
			const itemIds = data.items.map((item) => item.id);
			const appendedItemId = itemIds.pop();
			if (!appendedItemId) return;
			const currentIndex = itemIds.findIndex(
				(itemId) => itemId === currentQueueItemIdRef.current,
			);
			itemIds.splice(
				currentIndex >= 0 ? currentIndex + 1 : itemIds.length,
				0,
				appendedItemId,
			);
			await reorderQueue(itemIds);
		},
		[appendQueueItem, reorderQueue],
	);

	// Like Spotify and Apple Music, Clear removes only what is up next: the
	// playing Track keeps its queue item and played Tracks stay for Previous.
	const clearQueue = useCallback(async () => {
		await clearQueueAfter(
			session.source?.type === "track" ? currentQueueItemId : null,
		);
	}, [clearQueueAfter, session.source?.type, currentQueueItemId]);

	const value = useMemo<PlaybackContextValue>(
		() => ({
			queue,
			playbackSource: session.source,
			outputMode: session.outputMode,
			availableOutputDevices: session.availableOutputDevices,
			selectedOutputDevice: session.selectedOutputDevice,
			outputDeviceIssue: session.outputDeviceIssue,
			currentTrack,
			currentRadioStation,
			radioNowPlaying,
			isPlaying:
				session.status === "playing" || session.status === "reconnecting",
			isReconnecting: session.status === "reconnecting",
			currentTime: session.currentTime,
			duration: session.duration,
			bufferedEnd: session.bufferedEnd ?? null,
			volume: session.volume,
			playbackRate: session.playbackRate ?? 1,
			isGapless: session.isGapless ?? null,
			sleepTimer,
			abRepeat,
			shuffleEnabled: session.shuffleEnabled,
			repeatMode: session.repeatMode,
			playbackError: session.error
				? { ...session.error, cause: errorCause ?? session.error.cause }
				: null,
			errorRecovery,
			processingState: session.processing ?? null,
			playbackTelemetry: session.telemetry ?? null,
			queueConflict,
			playTrack,
			playRadioStation,
			playRadioCatalogPreview,
			queueTracks,
			playQueueIndex,
			playNext,
			navigatePrevious: () =>
				session.currentTime > PREVIOUS_RESTARTS_AFTER_SECONDS
					? engine.seek(0)
					: engine.previous(),
			navigateNext: () => engine.next(),
			togglePlay: () => engine.togglePlay(),
			toggleShuffle: () => engine.toggleShuffle(),
			cycleRepeatMode: () => engine.cycleRepeatMode(),
			seek: (seconds) => engine.seek(seconds),
			setVolume: (value) => engine.setVolume(value),
			setPlaybackRate: (rate) => engine.setPlaybackRate?.(rate),
			setTransitionFade: (milliseconds) =>
				engine.setTransitionFade?.(milliseconds),
			addToQueue: (trackId) => queueTracks([trackId]),
			setProcessingProfile: (profile) => engine.setProcessingProfile?.(profile),
			setReplayGainMode: (mode) => engine.setReplayGainMode?.(mode),
			setEqualizerPreset: (preset) => engine.setEqualizerPreset?.(preset),
			setEqualizerGain: (index, value) =>
				engine.setEqualizerGain?.(index, value),
			refreshOutputDevices: () => engine.refreshOutputDevices?.(),
			selectDirectAlsaOutput: (deviceId) =>
				engine.selectDirectAlsaOutput?.(deviceId),
			selectExclusiveOutput: () => engine.selectExclusiveOutput?.(),
			fallbackToSystemOutput: () => engine.fallbackToSystemOutput?.(),
			enableAdaptiveSystemRate: () => engine.enableAdaptiveSystemRate?.(),
			removeFromQueue,
			reorderQueue,
			clearQueue,
			refreshQueue,
			stopPlayback: () => engine.stop(),
			getAlbumCoverUrl: (albumId, size) =>
				apiRef.current.getAlbumCoverUrl(albumId, size),
			getTrack,
			getTrackLyrics,
			getTrackWaveform,
		}),
		[
			queue,
			queueConflict,
			currentTrack,
			currentRadioStation,
			radioNowPlaying,
			session,
			errorCause,
			errorRecovery,
			sleepTimer,
			abRepeat,
			getTrackWaveform,
			getTrack,
			getTrackLyrics,
			playTrack,
			playRadioStation,
			playRadioCatalogPreview,
			queueTracks,
			playQueueIndex,
			playNext,
			engine,
			removeFromQueue,
			reorderQueue,
			clearQueue,
			refreshQueue,
		],
	);

	const playlistLibrary = useMemo<PlaylistLibraryApi>(
		() => ({
			listPlaylists: () => apiRef.current.listPlaylists(),
			getPlaylist: (playlistId) => apiRef.current.getPlaylist(playlistId),
			createPlaylist: (name) => apiRef.current.createPlaylist(name),
			addPlaylistTrack: (playlistId, trackId) =>
				apiRef.current.addPlaylistTrack(playlistId, trackId),
			removePlaylistTrack: (playlistId, trackId) =>
				apiRef.current.removePlaylistTrack(playlistId, trackId),
		}),
		[],
	);

	return (
		<PlaybackContext.Provider value={value}>
			<PlaylistLibraryContext.Provider value={playlistLibrary}>
				{children}
			</PlaylistLibraryContext.Provider>
		</PlaybackContext.Provider>
	);
}

function queuePlaybackSources(
	queue: QueueItem[],
	api: PlaybackAssetApi,
): PlaybackSource[] {
	return queue.map((item) => ({
		type: "track",
		track: item.track,
		playbackUrl: api.getStreamUrl(item.trackId),
		queueItemId: item.id,
		artworkUrl: api.getAlbumCoverUrl(item.track.albumId, 512),
	}));
}

export function usePlayback() {
	const context = useContext(PlaybackContext);
	if (!context)
		throw new Error("usePlayback must be used within PlaybackProvider");
	return context;
}

export function usePlaylistLibrary() {
	const context = useContext(PlaylistLibraryContext);
	if (!context)
		throw new Error("usePlaylistLibrary must be used within PlaybackProvider");
	return context;
}

function getCurrentRadioStation(
	source: PlaybackSessionState["source"],
): RadioStation | null {
	if (source?.type === "radio-station") return source.station;
	if (source?.type !== "catalog-preview") return null;
	const result = source.result;
	return {
		id: `preview:${result.stationUuid}`,
		name: result.name,
		streamUrl: result.streamUrl,
		homepageUrl: result.homepageUrl,
		faviconUrl: result.faviconUrl,
		country: result.country,
		language: result.language,
		tags: result.tags,
		codec: result.codec,
		bitrate: result.bitrate,
		source: "radio-browser",
		externalId: result.stationUuid,
		isFavorite: false,
		position: -1,
	};
}
