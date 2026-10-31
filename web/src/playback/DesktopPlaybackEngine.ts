import type {
	EqualizerPreset,
	PlaybackEngine,
	PlaybackSessionListener,
	PlaybackSessionState,
	PlaybackSource,
	ProcessingProfile,
	ReplayGainMode,
} from "@repo/ui";
import { DEFAULT_PLAYBACK_SESSION_STATE } from "@repo/ui";
import { DESKTOP_EVENTS } from "@repo/desktop-bridge";
import { invoke, listen, type Unlisten } from "#/desktop/desktop-api";

type DesktopPlaybackBridge = {
	rendererReady(): Promise<PlaybackSessionState>;
	play(source?: PlaybackSource): Promise<PlaybackSessionState>;
	syncQueueContext(
		sources: PlaybackSource[],
		currentIndex: number | null,
	): Promise<PlaybackSessionState>;
	previous(): Promise<PlaybackSessionState>;
	next(): Promise<PlaybackSessionState>;
	pause(): Promise<PlaybackSessionState>;
	stop(): Promise<PlaybackSessionState>;
	togglePlay(): Promise<PlaybackSessionState>;
	seek(seconds: number): Promise<PlaybackSessionState>;
	setVolume(value: number): Promise<PlaybackSessionState>;
	setPlaybackRate(rate: number): Promise<PlaybackSessionState>;
	setStopAfterCurrent(enabled: boolean): Promise<PlaybackSessionState>;
	setTransitionFade(milliseconds: number): Promise<PlaybackSessionState>;
	setGapless(enabled: boolean): Promise<PlaybackSessionState>;
	setProcessingProfile(
		profile: ProcessingProfile,
	): Promise<PlaybackSessionState>;
	setReplayGainMode(mode: ReplayGainMode): Promise<PlaybackSessionState>;
	setEqualizerPreset(
		preset: Exclude<EqualizerPreset, "custom">,
	): Promise<PlaybackSessionState>;
	setEqualizerGain(index: number, value: number): Promise<PlaybackSessionState>;
	refreshOutputDevices(): Promise<PlaybackSessionState>;
	selectDirectAlsaOutput(deviceId: string): Promise<PlaybackSessionState>;
	selectExclusiveOutput(): Promise<PlaybackSessionState>;
	fallbackToSystemOutput(): Promise<PlaybackSessionState>;
	enableAdaptiveSystemRate(): Promise<PlaybackSessionState>;
	toggleShuffle(): Promise<PlaybackSessionState>;
	cycleRepeatMode(): Promise<PlaybackSessionState>;
	listen(listener: PlaybackSessionListener): Promise<Unlisten>;
};

const desktopPlaybackBridge: DesktopPlaybackBridge = {
	rendererReady: () => invoke("desktop_playback_renderer_ready"),
	play: (source) => invoke("desktop_playback_play", { source }),
	syncQueueContext: (sources, currentIndex) =>
		invoke("desktop_playback_sync_queue_context", { sources, currentIndex }),
	previous: () => invoke("desktop_playback_previous"),
	next: () => invoke("desktop_playback_next"),
	pause: () => invoke("desktop_playback_pause"),
	stop: () => invoke("desktop_playback_stop"),
	togglePlay: () => invoke("desktop_playback_toggle_play"),
	seek: (seconds) => invoke("desktop_playback_seek", { seconds }),
	setVolume: (value) => invoke("desktop_playback_set_volume", { value }),
	setPlaybackRate: (rate) =>
		invoke("desktop_playback_set_playback_rate", { rate }),
	setStopAfterCurrent: (enabled) =>
		invoke("desktop_playback_set_stop_after_current", { enabled }),
	setTransitionFade: (milliseconds) =>
		invoke("desktop_playback_set_transition_fade", { milliseconds }),
	setGapless: (enabled) => invoke("desktop_playback_set_gapless", { enabled }),
	setProcessingProfile: (profile) =>
		invoke("desktop_playback_set_processing_profile", { profile }),
	setReplayGainMode: (mode) =>
		invoke("desktop_playback_set_replay_gain", { mode }),
	setEqualizerPreset: (preset) =>
		invoke("desktop_playback_set_equalizer_preset", { preset }),
	setEqualizerGain: (index, value) =>
		invoke("desktop_playback_set_equalizer_gain", { index, value }),
	refreshOutputDevices: () => invoke("desktop_playback_refresh_output_devices"),
	selectDirectAlsaOutput: (deviceId) =>
		invoke("desktop_playback_select_direct_alsa_output", { deviceId }),
	selectExclusiveOutput: () =>
		invoke("desktop_playback_select_exclusive_output"),
	fallbackToSystemOutput: () =>
		invoke("desktop_playback_fallback_to_system_output"),
	enableAdaptiveSystemRate: () =>
		invoke("desktop_playback_enable_adaptive_system_rate"),
	toggleShuffle: () => invoke("desktop_playback_toggle_shuffle"),
	cycleRepeatMode: () => invoke("desktop_playback_cycle_repeat_mode"),
	listen: async (listener) =>
		listen<PlaybackSessionState>(DESKTOP_EVENTS.playbackState, listener),
};

export class DesktopPlaybackEngine implements PlaybackEngine {
	private state: PlaybackSessionState = { ...DEFAULT_PLAYBACK_SESSION_STATE };
	private readonly listeners = new Set<PlaybackSessionListener>();
	private unlisten: Unlisten | null = null;
	private isDestroyed = false;
	private commandRevision = 0;
	private stateRevision = 0;
	private isRefreshing = false;
	private readonly handleFocus = () => {
		void this.refreshSession();
	};
	private readonly handleVisibilityChange = () => {
		if (document.visibilityState === "visible") void this.refreshSession();
	};

	constructor(
		private readonly bridge: DesktopPlaybackBridge = desktopPlaybackBridge,
	) {
		void this.initialize();
	}

	getState() {
		return this.state;
	}

	subscribe(listener: PlaybackSessionListener) {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	async play(source?: PlaybackSource) {
		this.commandRevision += 1;
		try {
			this.update(await this.bridge.play(source));
		} catch (error) {
			this.updateError(error);
			throw error;
		}
	}

	async syncQueueContext(
		sources: PlaybackSource[],
		currentIndex: number | null,
	) {
		this.commandRevision += 1;
		try {
			this.update(await this.bridge.syncQueueContext(sources, currentIndex));
		} catch (error) {
			this.updateError(error);
			throw error;
		}
	}

	previous() {
		this.runCommand(() => this.bridge.previous());
	}

	next() {
		this.runCommand(() => this.bridge.next());
	}

	pause() {
		this.runCommand(() => this.bridge.pause());
	}

	stop() {
		this.runCommand(() => this.bridge.stop());
	}

	togglePlay() {
		this.runCommand(() => this.bridge.togglePlay());
	}

	seek(seconds: number) {
		this.runCommand(() => this.bridge.seek(seconds));
	}

	setVolume(value: number) {
		this.runCommand(() => this.bridge.setVolume(value));
	}

	setPlaybackRate(rate: number) {
		this.runCommand(() => this.bridge.setPlaybackRate(rate));
	}

	setStopAfterCurrent(enabled: boolean) {
		this.runCommand(() => this.bridge.setStopAfterCurrent(enabled));
	}

	setTransitionFade(milliseconds: number) {
		this.runCommand(() => this.bridge.setTransitionFade(milliseconds));
	}

	setGapless(enabled: boolean) {
		this.runCommand(() => this.bridge.setGapless(enabled));
	}

	setProcessingProfile(profile: ProcessingProfile) {
		this.runCommand(() => this.bridge.setProcessingProfile(profile));
	}

	setReplayGainMode(mode: ReplayGainMode) {
		this.runCommand(() => this.bridge.setReplayGainMode(mode));
	}

	setEqualizerPreset(preset: Exclude<EqualizerPreset, "custom">) {
		this.runCommand(() => this.bridge.setEqualizerPreset(preset));
	}

	setEqualizerGain(index: number, value: number) {
		this.runCommand(() => this.bridge.setEqualizerGain(index, value));
	}

	refreshOutputDevices() {
		this.runCommand(() => this.bridge.refreshOutputDevices());
	}

	selectDirectAlsaOutput(deviceId: string) {
		this.runCommand(() => this.bridge.selectDirectAlsaOutput(deviceId));
	}

	selectExclusiveOutput() {
		this.runCommand(() => this.bridge.selectExclusiveOutput());
	}

	fallbackToSystemOutput() {
		this.runCommand(() => this.bridge.fallbackToSystemOutput());
	}

	enableAdaptiveSystemRate() {
		this.runCommand(() => this.bridge.enableAdaptiveSystemRate());
	}

	toggleShuffle() {
		this.runCommand(() => this.bridge.toggleShuffle());
	}

	cycleRepeatMode() {
		this.runCommand(() => this.bridge.cycleRepeatMode());
	}

	destroy() {
		this.isDestroyed = true;
		window.removeEventListener("focus", this.handleFocus);
		document.removeEventListener(
			"visibilitychange",
			this.handleVisibilityChange,
		);
		this.unlisten?.();
		this.unlisten = null;
		this.listeners.clear();
	}

	private async initialize() {
		try {
			const unlisten = await this.bridge.listen((state) => this.update(state));
			if (this.isDestroyed) {
				unlisten();
				return;
			}
			this.unlisten = unlisten;
			window.addEventListener("focus", this.handleFocus);
			document.addEventListener(
				"visibilitychange",
				this.handleVisibilityChange,
			);
			await this.refreshSession();
		} catch (error) {
			this.updateError(error);
		}
	}

	private async refreshSession() {
		if (this.isDestroyed || this.isRefreshing) return;
		this.isRefreshing = true;
		const commandRevision = this.commandRevision;
		const stateRevision = this.stateRevision;
		try {
			const state = await this.bridge.rendererReady();
			if (
				this.commandRevision === commandRevision &&
				this.stateRevision === stateRevision
			) {
				this.update(state);
			}
		} catch (error) {
			console.warn("Failed to refresh native playback session", { error });
			if (this.stateRevision === stateRevision) this.updateError(error);
		} finally {
			this.isRefreshing = false;
		}
	}

	private runCommand(command: () => Promise<PlaybackSessionState>) {
		this.commandRevision += 1;
		void this.run(command);
	}

	private async run(command: () => Promise<PlaybackSessionState>) {
		try {
			this.update(await command());
		} catch (error) {
			this.updateError(error);
		}
	}

	private update(state: PlaybackSessionState) {
		if (this.isDestroyed) return;
		this.stateRevision += 1;
		this.state = state;
		for (const listener of this.listeners) listener(state);
	}

	private updateError(error: unknown) {
		this.update({
			...this.state,
			status: "error",
			error: {
				code: "playback-failed",
				message: getErrorMessage(error),
			},
		});
	}
}

function getErrorMessage(error: unknown) {
	if (error instanceof Error) return error.message;
	if (typeof error === "string") return error;
	if (
		typeof error === "object" &&
		error !== null &&
		"message" in error &&
		typeof error.message === "string"
	) {
		return error.message;
	}
	return "Native playback failed";
}
