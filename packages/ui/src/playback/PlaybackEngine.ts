import type {
	ExternalTrack,
	RadioSearchResult,
	RadioStation,
	Track,
} from "@repo/api-client";
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

export type RepeatMode = "off" | "once" | "loop";

export type PlaybackSource =
	| {
			type: "track";
			track: Track;
			playbackUrl: string;
			queueItemId?: string;
			/** Cover URL for OS media controls; optional for older hosts. */
			artworkUrl?: string;
			/** Starts here instead of 0, when taking over the shared playhead. */
			startPositionSeconds?: number;
	  }
	| {
			type: "radio-station";
			station: RadioStation;
			playbackUrl: string;
			sourceUrl: string;
	  }
	| {
			type: "catalog-preview";
			result: RadioSearchResult;
			playbackUrl: string;
			sourceUrl: string;
	  }
	| {
			/**
			 * An External Track played through the Music Server: not live,
			 * seekable, never queued, favourited or restored. Both URLs are the
			 * server's stream path, so a `.m3u8` path selects HLS playback.
			 */
			type: "external-track";
			externalTrack: ExternalTrack;
			playbackUrl: string;
			sourceUrl: string;
			/** Cover URL for OS media controls, when the Track has artwork. */
			artworkUrl?: string;
	  };

export type PlaybackStatus =
	| "idle"
	| "playing"
	| "reconnecting"
	| "paused"
	| "ended"
	| "error";

export type PlaybackError = {
	code:
		| "playback-failed"
		/** The browser refused to start without a user gesture. */
		| "playback-blocked"
		| "mpv-crash-loop"
		| "mpv-restart-failed"
		| "mpv-restart-unavailable"
		| "mpv-recovery-state-unavailable"
		| "mpv-recovery-snapshot-failed"
		| "mpv-recovery-lifecycle-unavailable"
		| "mpv-recovery-transition-invalid";
	message: string;
	/** Filled in by the provider after probing the stream; absent until then. */
	cause?: PlaybackErrorCause;
};

export type PlaybackErrorCause = "file-missing" | "network" | "unknown";

export type PlaybackSessionState = {
	source: PlaybackSource | null;
	outputMode: OutputMode | null;
	availableOutputDevices: OutputDevice[];
	selectedOutputDevice: OutputDevice | null;
	outputDeviceIssue: OutputDeviceIssue | null;
	status: PlaybackStatus;
	currentTime: number;
	duration: number;
	/** End of the buffered range in seconds; null or absent when unknown. */
	bufferedEnd?: number | null;
	volume: number;
	/** Speed multiplier; absent means 1. */
	playbackRate?: number;
	/** Sleep timer "after this track": the engine ends instead of advancing. */
	stopAfterCurrent?: boolean;
	/** Whether the engine plays consecutive tracks without a gap; null when unknown. */
	isGapless?: boolean | null;
	shuffleEnabled: boolean;
	repeatMode: RepeatMode;
	error: PlaybackError | null;
	processing?: ProcessingState;
	telemetry?: PlaybackTelemetry;
	/**
	 * Shared Playback State (ADR 0040): present while this device follows
	 * another one's Player, for the "Playing on …" indicator.
	 */
	sharedPlayback?: { followingDeviceName: string | null } | undefined;
};

export type PlaybackSessionListener = (state: PlaybackSessionState) => void;
export type PlaybackNavigationDirection = "previous" | "next";
export type PlaybackNavigationListener = (
	direction: PlaybackNavigationDirection,
) => void;

export interface PlaybackEngine {
	getState(): PlaybackSessionState;
	subscribe(listener: PlaybackSessionListener): () => void;
	subscribeNavigation?(listener: PlaybackNavigationListener): () => void;
	syncQueueContext?(
		sources: PlaybackSource[],
		currentIndex: number | null,
	): Promise<void>;
	play(source?: PlaybackSource): Promise<void>;
	/**
	 * Call from a click: lets later playback start without one, for a player
	 * whose platform requires it. Starting a Room's Track comes after a request.
	 */
	unlockAudio?(): void;
	previous(): void;
	next(): void;
	pause(): void;
	stop(): void;
	togglePlay(): void;
	seek(seconds: number): void;
	setVolume(value: number): void;
	setPlaybackRate?(rate: number): void;
	setStopAfterCurrent?(enabled: boolean): void;
	/** Volume ramp around user-initiated transitions, in milliseconds; 0 disables. */
	setTransitionFade?(milliseconds: number): void;
	/** Natural Track boundaries without a gap, where the engine can. */
	setGapless?(enabled: boolean): void;
	setProcessingProfile?(profile: ProcessingProfile): void;
	setReplayGainMode?(mode: ReplayGainMode): void;
	setEqualizerPreset?(preset: Exclude<EqualizerPreset, "custom">): void;
	setEqualizerGain?(index: number, value: number): void;
	refreshOutputDevices?(): void;
	selectDirectAlsaOutput?(deviceId: string): void;
	selectExclusiveOutput?(): void;
	fallbackToSystemOutput?(): void;
	enableAdaptiveSystemRate?(): void;
	/**
	 * The engine reports to and obeys the Shared Playback State by itself, as
	 * the Desktop core does so that it keeps working with the window closed.
	 */
	readonly reportsSharedPlaybackState?: boolean;
	/**
	 * Suspends Shared Playback State reporting and following, as a Room or a
	 * Guest session does (ADR 0040): everything passes through to the local
	 * Player untouched until it is lifted.
	 */
	setSharedPlaybackSuspended?(suspended: boolean): void;
	/**
	 * The Queue as sources, for taking over its neighbouring item. Apart from
	 * `syncQueueContext`, whose presence means the engine advances the Queue.
	 */
	setSharedQueueSources?(sources: PlaybackSource[]): void;
	toggleShuffle(): void;
	cycleRepeatMode(): void;
	destroy(): void;
}

export const DEFAULT_PLAYBACK_SESSION_STATE: PlaybackSessionState = {
	source: null,
	outputMode: null,
	availableOutputDevices: [],
	selectedOutputDevice: null,
	outputDeviceIssue: null,
	status: "idle",
	currentTime: 0,
	duration: 0,
	volume: 0.8,
	shuffleEnabled: false,
	repeatMode: "off",
	error: null,
};
