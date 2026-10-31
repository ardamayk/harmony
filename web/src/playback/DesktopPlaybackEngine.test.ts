import type {
	PlaybackSessionListener,
	PlaybackSessionState,
	PlaybackSource,
} from "@repo/ui";
import { DEFAULT_PLAYBACK_SESSION_STATE } from "@repo/ui";
import { describe, expect, it, vi } from "vite-plus/test";
import { DesktopPlaybackEngine } from "./DesktopPlaybackEngine";

const trackSource: PlaybackSource = {
	type: "track",
	track: {
		id: "track-1",
		title: "Track 1",
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120000,
		format: "flac",
		genres: [],
	},
	playbackUrl: `http://127.0.0.1:43129/${"a".repeat(64)}/api/v1/tracks/track-1/stream`,
};

const radioSource: PlaybackSource = {
	type: "radio-station",
	station: {
		id: "station-1",
		name: "Station 1",
		streamUrl: "https://radio.example/live.mp3",
		tags: [],
		source: "manual",
		isFavorite: false,
		position: 0,
	},
	playbackUrl: `http://127.0.0.1:43129/${"b".repeat(64)}/api/v1/radio/stations/station-1/stream`,
	sourceUrl: "https://radio.example/live.mp3",
};

const catalogSource: PlaybackSource = {
	type: "catalog-preview",
	result: {
		stationUuid: "catalog-1",
		name: "Catalog 1",
		streamUrl: "https://catalog.example/live.m3u8",
		tags: [],
	},
	playbackUrl: `http://127.0.0.1:43129/${"c".repeat(64)}/api/v1/radio/preview/catalog-1/stream`,
	sourceUrl: "https://catalog.example/live.m3u8",
};

function createBridge() {
	let listener: PlaybackSessionListener | null = null;
	let state: PlaybackSessionState = { ...DEFAULT_PLAYBACK_SESSION_STATE };
	return {
		bridge: {
			rendererReady: vi.fn(async () => state),
			syncQueueContext: vi.fn(async () => state),
			previous: vi.fn(async () => state),
			next: vi.fn(async () => state),
			play: vi.fn(async (source?: PlaybackSource) => {
				state = {
					...state,
					source: source ?? state.source,
					status: "playing",
					duration: 120,
				};
				return state;
			}),
			pause: vi.fn(async () => state),
			stop: vi.fn(async () => state),
			togglePlay: vi.fn(async () => state),
			seek: vi.fn(async () => state),
			setVolume: vi.fn(async () => state),
			setPlaybackRate: vi.fn(async () => state),
			setStopAfterCurrent: vi.fn(async () => state),
			setGapless: vi.fn(async () => state),
			setTransitionFade: vi.fn(async () => state),
			setProcessingProfile: vi.fn(async () => state),
			setReplayGainMode: vi.fn(async () => state),
			setEqualizerPreset: vi.fn(async () => state),
			setEqualizerGain: vi.fn(async () => state),
			refreshOutputDevices: vi.fn(async () => state),
			selectDirectAlsaOutput: vi.fn(async () => state),
			selectExclusiveOutput: vi.fn(async () => state),
			fallbackToSystemOutput: vi.fn(async () => state),
			enableAdaptiveSystemRate: vi.fn(async () => state),
			toggleShuffle: vi.fn(async () => state),
			cycleRepeatMode: vi.fn(async () => state),
			listen: vi.fn(async (nextListener: PlaybackSessionListener) => {
				listener = nextListener;
				return () => {
					listener = null;
				};
			}),
		},
		emit(nextState: PlaybackSessionState) {
			listener?.(nextState);
		},
	};
}

describe("DesktopPlaybackEngine", () => {
	it("forwards previous and next controls to Rust-owned navigation", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		engine.previous();
		engine.next();

		await vi.waitFor(() => expect(native.bridge.next).toHaveBeenCalledOnce());
		expect(native.bridge.previous).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("forwards Processing Profile controls through the native bridge", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		engine.setProcessingProfile("processed");
		engine.setReplayGainMode("album");
		engine.setEqualizerPreset("vocal");
		engine.setEqualizerGain(3, 2.5);

		await vi.waitFor(() =>
			expect(native.bridge.setEqualizerGain).toHaveBeenCalledWith(3, 2.5),
		);
		expect(native.bridge.setProcessingProfile).toHaveBeenCalledWith(
			"processed",
		);
		expect(native.bridge.setReplayGainMode).toHaveBeenCalledWith("album");
		expect(native.bridge.setEqualizerPreset).toHaveBeenCalledWith("vocal");
		engine.destroy();
	});

	it("forwards Normal and automatic Exclusive controls through the native bridge", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		engine.selectExclusiveOutput();
		engine.fallbackToSystemOutput();

		await vi.waitFor(() =>
			expect(native.bridge.fallbackToSystemOutput).toHaveBeenCalledOnce(),
		);
		expect(native.bridge.selectExclusiveOutput).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("forwards Adaptive System Rate selection without confirmation payload", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		engine.enableAdaptiveSystemRate();

		await vi.waitFor(() =>
			expect(native.bridge.enableAdaptiveSystemRate).toHaveBeenCalledWith(),
		);
		engine.destroy();
	});
	it("syncs Queue context through the Rust-owned playback seam", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);
		await engine.syncQueueContext([trackSource], 0);

		expect(native.bridge.syncQueueContext).toHaveBeenCalledWith(
			[trackSource],
			0,
		);
		engine.destroy();
	});

	it("hydrates the Rust-owned session when the renderer becomes ready", async () => {
		const native = createBridge();
		native.bridge.rendererReady.mockResolvedValueOnce({
			...DEFAULT_PLAYBACK_SESSION_STATE,
			source: radioSource,
			outputMode: "system",
			status: "playing",
		});
		const engine = new DesktopPlaybackEngine(native.bridge);

		await vi.waitFor(() => {
			expect(engine.getState()).toMatchObject({
				source: radioSource,
				outputMode: "system",
				status: "playing",
			});
		});

		expect(native.bridge.listen).toHaveBeenCalledOnce();
		expect(native.bridge.rendererReady).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("refreshes background playback when the main window regains focus", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);
		await vi.waitFor(() =>
			expect(native.bridge.rendererReady).toHaveBeenCalledOnce(),
		);
		native.bridge.rendererReady.mockResolvedValue({
			...DEFAULT_PLAYBACK_SESSION_STATE,
			source: trackSource,
			status: "playing",
			currentTime: 42,
		});
		window.dispatchEvent(new Event("focus"));
		await vi.waitFor(() =>
			expect(engine.getState()).toMatchObject({
				source: trackSource,
				status: "playing",
				currentTime: 42,
			}),
		);
		engine.destroy();
		window.dispatchEvent(new Event("focus"));
		expect(native.bridge.rendererReady).toHaveBeenCalledTimes(2);
	});

	it("plays a Track and projects native timing events", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);
		const observedStates: PlaybackSessionState[] = [];
		engine.subscribe((state) => observedStates.push(state));

		await engine.play(trackSource);
		native.emit({
			...engine.getState(),
			currentTime: 15.5,
			duration: 121.25,
		});

		expect(native.bridge.play).toHaveBeenCalledWith(trackSource);
		expect(engine.getState()).toMatchObject({
			source: trackSource,
			status: "playing",
			currentTime: 15.5,
			duration: 121.25,
		});
		expect(observedStates.at(-1)?.currentTime).toBe(15.5);
		engine.destroy();
	});

	it("plays saved Radio Stations through the native bridge proxy URL", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		await engine.play(radioSource);

		expect(native.bridge.play).toHaveBeenCalledWith(radioSource);
		expect(engine.getState()).toMatchObject({
			source: radioSource,
			status: "playing",
		});
		engine.destroy();
	});

	it("plays Catalog Previews through the native bridge proxy URL", async () => {
		const native = createBridge();
		const engine = new DesktopPlaybackEngine(native.bridge);

		await engine.play(catalogSource);

		expect(native.bridge.play).toHaveBeenCalledWith(catalogSource);
		expect(engine.getState()).toMatchObject({
			source: catalogSource,
			status: "playing",
		});
		engine.destroy();
	});

	it("preserves actionable native playback errors", async () => {
		const native = createBridge();
		native.bridge.play.mockRejectedValue({
			code: "playback-failed",
			message: "Pinned mpv could not decode this Track.",
		});
		const engine = new DesktopPlaybackEngine(native.bridge);

		await expect(engine.play(trackSource)).rejects.toMatchObject({
			code: "playback-failed",
		});

		expect(engine.getState()).toMatchObject({
			status: "error",
			error: {
				code: "playback-failed",
				message: "Pinned mpv could not decode this Track.",
			},
		});
		engine.destroy();
	});
});
