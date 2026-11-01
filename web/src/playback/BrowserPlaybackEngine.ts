import type {
	PlaybackEngine,
	PlaybackNavigationListener,
	PlaybackSessionListener,
	PlaybackSessionState,
	PlaybackSource,
} from "@repo/ui";
import { DEFAULT_PLAYBACK_SESSION_STATE } from "@repo/ui";
import { recordClientMetric } from "#/lib/client-metrics";

export interface BrowserPlaybackMedia extends EventTarget {
	currentTime: number;
	duration: number;
	paused: boolean;
	src: string;
	volume: number;
	/** Optional: TimeRanges-like buffered ranges, read on `progress` events. */
	buffered?: {
		length: number;
		start(i: number): number;
		end(i: number): number;
	};
	/** Optional: speed multiplier; browsers preserve pitch by default. */
	playbackRate?: number;
	/** Optional: only iOS playback sets this; see handleLoopWrap. */
	loop?: boolean;
	canPlayType(type: string): string;
	play(): Promise<void>;
	pause(): void;
	load(): void;
	removeAttribute(name: string): void;
}

type BrowserHls = {
	attachMedia(media: BrowserPlaybackMedia): void;
	destroy(): void;
	loadSource(url: string): void;
	onFatalError?(listener: () => void): void;
};

type BrowserHlsFactory = {
	isSupported(): boolean;
	create(): BrowserHls;
};

type BrowserHlsFactoryLoader = () => Promise<BrowserHlsFactory>;

/** The subset of navigator.mediaSession the engine drives; injectable for tests. */
interface BrowserMediaSession {
	metadata: MediaMetadata | null;
	playbackState: MediaSessionPlaybackState;
	setActionHandler(
		action: MediaSessionAction,
		handler: MediaSessionActionHandler | null,
	): void;
	setPositionState?(state?: MediaPositionState): void;
}

type BrowserPlaybackEngineOptions = {
	createMedia?: () => BrowserPlaybackMedia;
	hls?: BrowserHlsFactory;
	loadHls?: BrowserHlsFactoryLoader;
	/** Pass null to opt out; defaults to navigator.mediaSession when present. */
	mediaSession?: BrowserMediaSession | null;
	/** Defaults to detecting iPhone and iPadOS. */
	isIos?: boolean;
};

const MEDIA_SESSION_ACTIONS: MediaSessionAction[] = [
	"play",
	"pause",
	"stop",
	"previoustrack",
	"nexttrack",
	"seekto",
	"seekbackward",
	"seekforward",
];
const MEDIA_SESSION_SEEK_OFFSET_SECONDS = 10;

// Wide, because only a wrap moves playback backward unprompted, and timeupdate
// can be sparse in the background and at double speed.
const LOOP_WRAP_EDGE_SECONDS = 5;
// How far an ended Track may replay while it waits for the next source.
const ENDED_REPLAY_LIMIT_SECONDS = 1;

const LIVE_RECONNECT_DELAYS_MS = [1000, 2000, 4000, 8000, 15000] as const;
const LIVE_STALL_TIMEOUT_MS = 10000;
const LIVE_STABILITY_RESET_MS = 30000;

let defaultHlsFactoryPromise: Promise<BrowserHlsFactory> | null = null;

function loadDefaultHlsFactory() {
	defaultHlsFactoryPromise ??= import("hls.js").then(({ default: Hls }) => ({
		isSupported: () => Hls.isSupported(),
		create: () => {
			const hls = new Hls();
			return {
				attachMedia: (media) => hls.attachMedia(media as HTMLMediaElement),
				destroy: () => hls.destroy(),
				loadSource: (url) => hls.loadSource(url),
				onFatalError: (listener) => {
					hls.on(Hls.Events.ERROR, (_event, data) => {
						if (data.fatal) listener();
					});
				},
			};
		},
	}));
	return defaultHlsFactoryPromise;
}

export class BrowserPlaybackEngine implements PlaybackEngine {
	private readonly media: BrowserPlaybackMedia;
	private hlsFactory: BrowserHlsFactory | null;
	private readonly loadHlsFactory: BrowserHlsFactoryLoader;
	private readonly listeners = new Set<PlaybackSessionListener>();
	private readonly navigationListeners = new Set<PlaybackNavigationListener>();
	private state: PlaybackSessionState = {
		...DEFAULT_PLAYBACK_SESSION_STATE,
		// The audio element reloads between tracks, so there is always a gap.
		isGapless: false,
	};
	private hls: BrowserHls | null = null;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private stallTimer: ReturnType<typeof setTimeout> | null = null;
	private stabilityTimer: ReturnType<typeof setTimeout> | null = null;
	private reconnectAttempt = 0;
	private sourceRevision = 0;
	private hasStartedLivePlayback = false;
	private isReconnectAttemptRunning = false;
	private readonly mediaSession: BrowserMediaSession | null;
	private readonly isIos: boolean;
	/** The element's position at the last timeupdate or seek, to spot a wrap. */
	private lastMediaTime = 0;

	constructor(options: BrowserPlaybackEngineOptions = {}) {
		this.media = options.createMedia?.() ?? new Audio();
		this.hlsFactory = options.hls ?? null;
		this.loadHlsFactory = options.loadHls ?? loadDefaultHlsFactory;
		this.isIos = options.isIos ?? detectIos();
		this.mediaSession =
			options.mediaSession === undefined
				? defaultMediaSession()
				: options.mediaSession;
		this.media.volume = this.state.volume;
		if (this.isIos) declareAudioPlayback();
		this.addMediaListeners();
		this.registerMediaSessionHandlers();
	}

	getState() {
		return this.state;
	}

	subscribe(listener: PlaybackSessionListener) {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	subscribeNavigation(listener: PlaybackNavigationListener) {
		this.navigationListeners.add(listener);
		return () => this.navigationListeners.delete(listener);
	}

	previous() {
		this.publishNavigation("previous");
	}

	next() {
		this.publishNavigation("next");
	}

	async play(source?: PlaybackSource) {
		const nextSource = source ?? this.state.source;
		if (!nextSource) return;

		if (source) {
			this.cancelLiveReconnect();
			this.sourceRevision += 1;
			this.hasStartedLivePlayback = false;
			this.update({
				source,
				status: "paused",
				currentTime: 0,
				bufferedEnd: null,
				duration: sourceDurationSeconds(source),
				error: null,
			});
			this.applyMediaSessionMetadata(source);
		}

		try {
			const startedAt = performance.now();
			if (source) await this.setMediaSource(source);
			await this.media.play();
			this.update({ status: "playing", error: null });
			if (source) {
				recordClientMetric(
					"client.playback.start",
					performance.now() - startedAt,
				);
			}
		} catch (error) {
			// load() or pause() cut this start short, and whatever called them
			// owns the state now: the Track did not fail.
			if (hasErrorName(error, "AbortError")) return;
			this.update({
				status: "error",
				error: {
					code: "playback-failed",
					message: getErrorMessage(error),
				},
			});
			throw error;
		}
	}

	unlockAudio() {
		if (this.state.source) return;
		// Browsers remember an element that a click tried to play. Without a
		// source this attempt fails, which is all it needs to do.
		void this.media.play().catch(() => undefined);
	}

	pause() {
		this.cancelLiveReconnect();
		this.sourceRevision += 1;
		this.media.pause();
		if (this.state.source) this.update({ status: "paused" });
	}

	stop() {
		this.cancelLiveReconnect();
		this.sourceRevision += 1;
		this.media.pause();
		this.destroyHls();
		this.media.removeAttribute("src");
		this.media.load();
		this.update({
			source: null,
			status: "idle",
			currentTime: 0,
			duration: 0,
			error: null,
		});
	}

	togglePlay() {
		if (
			this.state.status === "playing" ||
			this.state.status === "reconnecting"
		) {
			this.pause();
			return;
		}
		if (this.media.paused) {
			void this.play().catch(() => undefined);
			return;
		}
		this.pause();
	}

	seek(seconds: number) {
		this.lastMediaTime = seconds;
		this.media.currentTime = seconds;
		this.update({ currentTime: seconds });
	}

	setVolume(value: number) {
		const volume = Math.min(1, Math.max(0, value));
		this.media.volume = volume;
		this.update({ volume });
	}

	setPlaybackRate(rate: number) {
		const playbackRate = Math.min(2, Math.max(0.5, rate));
		if ("playbackRate" in this.media) this.media.playbackRate = playbackRate;
		this.update({ playbackRate });
	}

	setStopAfterCurrent(enabled: boolean) {
		this.update({ stopAfterCurrent: enabled });
	}

	toggleShuffle() {
		this.update({ shuffleEnabled: !this.state.shuffleEnabled });
	}

	cycleRepeatMode() {
		const repeatMode =
			this.state.repeatMode === "off"
				? "once"
				: this.state.repeatMode === "once"
					? "loop"
					: "off";
		this.update({ repeatMode });
	}

	destroy() {
		this.cancelLiveReconnect();
		this.sourceRevision += 1;
		this.media.pause();
		this.destroyHls();
		this.removeMediaListeners();
		this.unregisterMediaSessionHandlers();
		this.media.removeAttribute("src");
		this.media.load();
		this.listeners.clear();
		this.navigationListeners.clear();
	}

	private registerMediaSessionHandlers() {
		const session = this.mediaSession;
		if (!session) return;
		const handlers: Partial<
			Record<MediaSessionAction, MediaSessionActionHandler | null>
		> = {
			play: () => void this.play().catch(() => undefined),
			pause: () => this.pause(),
			stop: () => this.stop(),
			previoustrack: () => this.previous(),
			nexttrack: () => this.next(),
			seekto: (details) => {
				if (typeof details.seekTime === "number") this.seek(details.seekTime);
			},
			seekbackward: (details) =>
				this.seek(
					Math.max(
						0,
						this.state.currentTime -
							(details.seekOffset ?? MEDIA_SESSION_SEEK_OFFSET_SECONDS),
					),
				),
			seekforward: (details) =>
				this.seek(
					this.state.currentTime +
						(details.seekOffset ?? MEDIA_SESSION_SEEK_OFFSET_SECONDS),
				),
		};
		if (this.isIos) {
			// iOS shows these as 10-second skips in place of previous and next.
			handlers.seekbackward = null;
			handlers.seekforward = null;
		}
		for (const action of MEDIA_SESSION_ACTIONS) {
			const handler = handlers[action];
			if (handler === undefined) continue;
			try {
				session.setActionHandler(action, handler);
			} catch {
				// Browsers throw for actions they do not support; skip them.
			}
		}
	}

	private unregisterMediaSessionHandlers() {
		const session = this.mediaSession;
		if (!session) return;
		for (const action of MEDIA_SESSION_ACTIONS) {
			try {
				session.setActionHandler(action, null);
			} catch {
				// Unsupported action; nothing was registered.
			}
		}
		session.metadata = null;
	}

	private applyMediaSessionMetadata(source: PlaybackSource) {
		const session = this.mediaSession;
		if (!session) return;
		session.metadata = createMediaMetadata(mediaMetadataFor(source));
	}

	private syncMediaSession() {
		const session = this.mediaSession;
		if (!session) return;
		if (!this.state.source) session.metadata = null;
		session.playbackState =
			this.state.status === "playing" || this.state.status === "reconnecting"
				? "playing"
				: this.state.source
					? "paused"
					: "none";
		if (!session.setPositionState) return;
		const duration = this.state.duration;
		if (!Number.isFinite(duration) || duration <= 0) {
			try {
				session.setPositionState();
			} catch {
				// Some browsers reject clearing; ignore.
			}
			return;
		}
		try {
			session.setPositionState({
				duration,
				playbackRate: this.state.playbackRate ?? 1,
				position: Math.min(duration, Math.max(0, this.state.currentTime)),
			});
		} catch {
			// Position outside the duration during a source swap; ignore.
		}
	}

	private publishNavigation(direction: "previous" | "next") {
		for (const listener of this.navigationListeners) listener(direction);
	}

	private readonly handleTimeUpdate = () => {
		if (isLiveSource(this.state.source)) this.clearStallTimer();
		const previousTime = this.lastMediaTime;
		const currentTime = this.media.currentTime;
		this.lastMediaTime = currentTime;
		if (this.media.loop && this.state.source) {
			if (this.state.status === "ended") {
				// Publishing here would repeat "ended" and advance the Queue again.
				if (currentTime >= ENDED_REPLAY_LIMIT_SECONDS) this.media.pause();
				return;
			}
			if (this.isLoopWrap(previousTime, currentTime)) {
				this.handleLoopWrap();
				return;
			}
		}
		this.update({ currentTime });
	};

	private readonly handleProgress = () => {
		const bufferedEnd = bufferedEndFor(this.media, this.state.currentTime);
		if (bufferedEnd !== this.state.bufferedEnd) this.update({ bufferedEnd });
	};

	private readonly handleDurationChange = () => {
		if (Number.isFinite(this.media.duration) && this.media.duration > 0) {
			this.update({ duration: this.media.duration });
		}
	};

	private readonly handlePlay = () => {
		if (this.state.status !== "reconnecting") {
			this.update({ status: "playing" });
		}
	};
	private readonly handlePlaying = () => {
		this.clearStallTimer();
		if (isLiveSource(this.state.source)) this.hasStartedLivePlayback = true;
		// iOS ignores handlers registered before the element is Now Playing.
		if (this.isIos) this.registerMediaSessionHandlers();
		// A wrap can rebuffer, and the replaying element is not a resumed Track.
		if (!(this.media.loop && this.state.status === "ended")) {
			this.update({ status: "playing" });
		}
		if (this.reconnectAttempt > 0) {
			this.clearStabilityTimer();
			this.stabilityTimer = setTimeout(() => {
				this.stabilityTimer = null;
				this.reconnectAttempt = 0;
			}, LIVE_STABILITY_RESET_MS);
		}
	};
	private readonly handlePause = () => {
		if (this.state.status !== "ended") this.update({ status: "paused" });
	};

	private readonly handleEnded = () => {
		this.lastMediaTime = 0;
		this.finishSource();
	};

	private isLoopWrap(previousTime: number, currentTime: number) {
		return (
			currentTime < previousTime &&
			currentTime < LOOP_WRAP_EDGE_SECONDS &&
			previousTime > this.state.duration - LOOP_WRAP_EDGE_SECONDS
		);
	}

	// WebKit bug 261858: iOS closes the audio session at a natural end in the
	// background and will not reopen it until foregrounded, so the next Track
	// stays paused. Looping keeps the session open; this wrap replaces `ended`.
	// Do not pause for a Track that ended: its next source needs the live session.
	private handleLoopWrap() {
		const { source, stopAfterCurrent } = this.state;
		this.finishSource();
		// Nothing follows these, so nothing else would stop the replay.
		if (stopAfterCurrent || source?.type === "external-track") {
			this.media.pause();
		}
	}

	private finishSource() {
		if (isLiveSource(this.state.source) && this.hasStartedLivePlayback) {
			this.scheduleLiveReconnect();
			return;
		}
		if (this.state.stopAfterCurrent) {
			// Consume the timer without publishing an end that advances the queue.
			this.update({ status: "paused", stopAfterCurrent: false });
			return;
		}
		// An External Track never repeats; the stored repeat mode stays for the
		// next Track.
		if (
			this.state.source?.type !== "external-track" &&
			(this.state.repeatMode === "once" || this.state.repeatMode === "loop")
		) {
			const repeatMode =
				this.state.repeatMode === "once" ? "off" : this.state.repeatMode;
			this.media.currentTime = 0;
			this.update({ currentTime: 0, status: "playing", repeatMode });
			void this.play().catch(() => undefined);
			return;
		}
		this.update({ status: "ended" });
	}

	private readonly handleMediaError = () => {
		if (this.isReconnectAttemptRunning) return;
		if (isLiveSource(this.state.source) && this.hasStartedLivePlayback) {
			this.scheduleLiveReconnect();
			return;
		}
		this.update({
			status: "error",
			error: {
				code: "playback-failed",
				message: "Playback failed",
			},
		});
	};
	private readonly handleStalled = () => {
		if (
			this.stallTimer ||
			!isLiveSource(this.state.source) ||
			!this.hasStartedLivePlayback
		) {
			return;
		}
		this.stallTimer = setTimeout(() => {
			this.stallTimer = null;
			this.scheduleLiveReconnect();
		}, LIVE_STALL_TIMEOUT_MS);
	};

	private addMediaListeners() {
		this.media.addEventListener("timeupdate", this.handleTimeUpdate);
		this.media.addEventListener("durationchange", this.handleDurationChange);
		this.media.addEventListener("loadedmetadata", this.handleDurationChange);
		this.media.addEventListener("play", this.handlePlay);
		this.media.addEventListener("playing", this.handlePlaying);
		this.media.addEventListener("pause", this.handlePause);
		this.media.addEventListener("ended", this.handleEnded);
		this.media.addEventListener("error", this.handleMediaError);
		this.media.addEventListener("stalled", this.handleStalled);
		this.media.addEventListener("waiting", this.handleStalled);
		this.media.addEventListener("progress", this.handleProgress);
	}

	private removeMediaListeners() {
		this.media.removeEventListener("timeupdate", this.handleTimeUpdate);
		this.media.removeEventListener("durationchange", this.handleDurationChange);
		this.media.removeEventListener("loadedmetadata", this.handleDurationChange);
		this.media.removeEventListener("play", this.handlePlay);
		this.media.removeEventListener("playing", this.handlePlaying);
		this.media.removeEventListener("pause", this.handlePause);
		this.media.removeEventListener("ended", this.handleEnded);
		this.media.removeEventListener("error", this.handleMediaError);
		this.media.removeEventListener("stalled", this.handleStalled);
		this.media.removeEventListener("waiting", this.handleStalled);
		this.media.removeEventListener("progress", this.handleProgress);
	}

	private async setMediaSource(source: PlaybackSource) {
		this.destroyHls();
		this.lastMediaTime = 0;
		if (this.isIos) {
			// Tearing the source down between Tracks can end the audio session.
			this.media.loop =
				!isLiveSource(source) && sourceDurationSeconds(source) > 0;
		} else {
			this.media.removeAttribute("src");
			this.media.load();
		}
		this.restorePlaybackRate();
		const sourceUrl =
			source.type === "track" ? source.playbackUrl : source.sourceUrl;
		const useHlsJs =
			isHlsStream(sourceUrl) && !(this.isIos && canPlayNativeHls(this.media));
		if (useHlsJs) {
			this.hlsFactory ??= await this.loadHlsFactory();
		}
		if (useHlsJs && this.hlsFactory?.isSupported()) {
			this.hls = this.hlsFactory.create();
			this.hls.onFatalError?.(this.handleMediaError);
			this.hls.loadSource(source.playbackUrl);
			this.hls.attachMedia(this.media);
			return;
		}
		this.media.src = source.playbackUrl;
		// Assigning src is a load too, and here no later step restores the rate.
		if (this.isIos) this.restorePlaybackRate();
	}

	// load() resets the element's rate in some browsers; keep the setting.
	private restorePlaybackRate() {
		if ("playbackRate" in this.media && this.state.playbackRate) {
			this.media.playbackRate = this.state.playbackRate;
		}
	}

	private destroyHls() {
		this.hls?.destroy();
		this.hls = null;
	}

	private scheduleLiveReconnect() {
		if (
			this.reconnectTimer ||
			this.isReconnectAttemptRunning ||
			!isLiveSource(this.state.source)
		) {
			return;
		}
		this.clearStabilityTimer();
		const source = this.state.source;
		const sourceRevision = this.sourceRevision;
		const delayIndex = Math.min(
			this.reconnectAttempt,
			LIVE_RECONNECT_DELAYS_MS.length - 1,
		);
		const delay = LIVE_RECONNECT_DELAYS_MS[delayIndex];
		this.reconnectAttempt += 1;
		this.update({ status: "reconnecting", error: null });
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			void this.reconnectLiveSource(source, sourceRevision);
		}, delay);
	}

	private async reconnectLiveSource(
		source: PlaybackSource,
		sourceRevision: number,
	) {
		if (
			this.sourceRevision !== sourceRevision ||
			this.state.source !== source ||
			this.state.status !== "reconnecting"
		) {
			return;
		}
		this.isReconnectAttemptRunning = true;
		let shouldRetry = false;
		try {
			await this.setMediaSource(source);
			if (
				this.sourceRevision !== sourceRevision ||
				this.state.source !== source ||
				this.state.status !== "reconnecting"
			) {
				return;
			}
			await this.media.play();
		} catch (error) {
			console.warn("Live playback reconnect attempt failed", {
				sourceType: source.type,
				attempt: this.reconnectAttempt,
				errorType: error instanceof Error ? error.name : typeof error,
				errorMessage: sanitizeReconnectErrorMessage(error),
			});
			shouldRetry =
				this.sourceRevision === sourceRevision &&
				this.state.source === source &&
				this.state.status === "reconnecting";
		} finally {
			this.isReconnectAttemptRunning = false;
		}
		if (shouldRetry) this.scheduleLiveReconnect();
	}

	private cancelLiveReconnect() {
		if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
		this.reconnectTimer = null;
		this.clearStallTimer();
		this.clearStabilityTimer();
		this.reconnectAttempt = 0;
	}

	private clearStallTimer() {
		if (this.stallTimer) clearTimeout(this.stallTimer);
		this.stallTimer = null;
	}

	private clearStabilityTimer() {
		if (this.stabilityTimer) clearTimeout(this.stabilityTimer);
		this.stabilityTimer = null;
	}

	private update(next: Partial<PlaybackSessionState>) {
		this.state = { ...this.state, ...next };
		for (const listener of this.listeners) listener(this.state);
		if (
			"status" in next ||
			"currentTime" in next ||
			"duration" in next ||
			"playbackRate" in next ||
			"source" in next
		) {
			this.syncMediaSession();
		}
	}
}

function detectIos() {
	if (typeof navigator === "undefined") return false;
	return (
		/^(iPhone|iPad|iPod)/.test(navigator.platform) ||
		// iPadOS presents itself as a Mac; only the touch points give it away.
		(navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1)
	);
}

/** Declares this page a media player, which iOS uses to route and keep audio. */
function declareAudioPlayback() {
	if (typeof navigator === "undefined") return;
	try {
		const { audioSession } = navigator as Navigator & {
			audioSession?: { type: string };
		};
		if (audioSession) audioSession.type = "playback";
	} catch {
		// Playback works without it, so it must not break construction.
	}
}

function defaultMediaSession(): BrowserMediaSession | null {
	if (typeof navigator === "undefined") return null;
	const session = (navigator as Navigator & { mediaSession?: MediaSession })
		.mediaSession;
	return session ?? null;
}

type MediaMetadataFields = {
	title: string;
	artist: string;
	album: string;
	artwork: { src: string }[];
};

function mediaMetadataFor(source: PlaybackSource): MediaMetadataFields {
	if (source.type === "track") {
		return {
			title: source.track.title,
			artist: source.track.artistName,
			album: source.track.albumTitle ?? "",
			artwork: source.artworkUrl ? [{ src: source.artworkUrl }] : [],
		};
	}
	if (source.type === "external-track") {
		return {
			title: source.externalTrack.title,
			artist: source.externalTrack.artistName ?? "",
			album: "",
			artwork: source.artworkUrl ? [{ src: source.artworkUrl }] : [],
		};
	}
	const radio =
		source.type === "radio-station" ? source.station : source.result;
	return {
		title: radio.name,
		artist: "Live radio",
		album: "",
		artwork: radio.faviconUrl ? [{ src: radio.faviconUrl }] : [],
	};
}

function createMediaMetadata(
	fields: MediaMetadataFields,
): MediaMetadata | null {
	const MetadataConstructor = (
		globalThis as { MediaMetadata?: typeof MediaMetadata }
	).MediaMetadata;
	if (!MetadataConstructor) return null;
	try {
		return new MetadataConstructor(fields);
	} catch {
		return null;
	}
}

// Until the media element reports its own duration, the Track's or External
// Track's known length keeps the seek bar usable; live radio has none.
function sourceDurationSeconds(source: PlaybackSource) {
	const durationMs =
		source.type === "track"
			? source.track.durationMs
			: source.type === "external-track"
				? source.externalTrack.durationMs
				: null;
	return durationMs && durationMs > 0 ? durationMs / 1000 : 0;
}

function isLiveSource(
	source: PlaybackSource | null,
): source is Extract<
	PlaybackSource,
	{ type: "radio-station" | "catalog-preview" }
> {
	return source?.type === "radio-station" || source?.type === "catalog-preview";
}

/** play() rejects with a named DOMException, which is not always an Error. */
function hasErrorName(error: unknown, name: string): boolean {
	return (error as { name?: unknown } | null | undefined)?.name === name;
}

function isHlsStream(url: string) {
	try {
		return new URL(url, window.location.href).pathname
			.toLowerCase()
			.endsWith(".m3u8");
	} catch {
		return url.toLowerCase().includes(".m3u8");
	}
}

function canPlayNativeHls(media: BrowserPlaybackMedia) {
	return Boolean(
		media.canPlayType("application/vnd.apple.mpegurl") ||
		media.canPlayType("application/x-mpegurl"),
	);
}

function getErrorMessage(error: unknown) {
	return error instanceof Error ? error.message : "Playback failed";
}

function sanitizeReconnectErrorMessage(error: unknown) {
	if (!(error instanceof Error)) return "Non-Error rejection";
	const urlPattern = /\bhttps?:\/\/\S+/gi;
	const maxMessageLength = 240;
	const message = error.message.replace(urlPattern, "[redacted-url]").trim();
	return (message || error.name).slice(0, maxMessageLength);
}

/**
 * End of the buffered range that contains the playhead, or the furthest
 * range end when none does; null when the media exposes no ranges.
 */
function bufferedEndFor(
	media: Pick<BrowserPlaybackMedia, "buffered">,
	currentTime: number,
): number | null {
	const ranges = media.buffered;
	if (!ranges || ranges.length === 0) return null;
	let furthest = 0;
	for (let index = 0; index < ranges.length; index += 1) {
		const start = ranges.start(index);
		const end = ranges.end(index);
		if (start <= currentTime && currentTime <= end) return end;
		furthest = Math.max(furthest, end);
	}
	return furthest;
}
