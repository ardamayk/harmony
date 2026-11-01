import { playbackEngineContract, trackSource } from "@repo/ui/playback/testing";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	BrowserPlaybackEngine,
	type BrowserPlaybackMedia,
} from "./BrowserPlaybackEngine";

class MemoryMedia extends EventTarget implements BrowserPlaybackMedia {
	buffered?: BrowserPlaybackMedia["buffered"];
	currentTime = 0;
	duration = 0;
	loop = false;
	paused = true;
	src = "";
	volume = 1;
	canPlayType = vi.fn(() => "");
	play = vi.fn(async () => {
		this.paused = false;
		this.dispatchEvent(new Event("play"));
		this.dispatchEvent(new Event("playing"));
	});
	pause = vi.fn(() => {
		this.paused = true;
		this.dispatchEvent(new Event("pause"));
	});
	load = vi.fn();
	removeAttribute = vi.fn((name: string) => {
		if (name === "src") this.src = "";
	});
}

let media: MemoryMedia;

function createCatalogPreviewSource(stationUuid: string) {
	const sourceUrl = `https://example.com/${stationUuid}.mp3`;
	return {
		type: "catalog-preview" as const,
		result: {
			stationUuid,
			name: stationUuid,
			streamUrl: sourceUrl,
			tags: [],
		},
		playbackUrl: `/api/v1/radio/preview/${stationUuid}/stream`,
		sourceUrl,
	};
}

function createRadioStationSource(id: string, sourceUrl?: string) {
	const streamUrl = sourceUrl ?? `https://example.com/${id}.mp3`;
	return {
		type: "radio-station" as const,
		station: {
			id,
			name: id,
			streamUrl,
			tags: [],
			source: "manual" as const,
			isFavorite: false,
			position: 0,
		},
		playbackUrl: `/api/v1/radio/stations/${id}/stream`,
		sourceUrl: streamUrl,
	};
}

const externalTrackSource = {
	type: "external-track" as const,
	externalTrack: {
		id: "external-1",
		title: "External Song",
		artistName: null,
		durationMs: 30000,
		webpageUrl: "https://soundcloud.com/uploader/external-song",
		extractor: "Soundcloud",
		hasArtwork: false,
	},
	playbackUrl: "/api/v1/external/tracks/external-1/stream",
	sourceUrl: "/api/v1/external/tracks/external-1/stream",
};

function reachTime(seconds: number) {
	media.currentTime = seconds;
	media.dispatchEvent(new Event("timeupdate"));
}

/** What a looping element does at the end of a Track: wrap, and keep playing. */
function wrapLoop(duration = 120) {
	reachTime(duration - 0.1);
	reachTime(0.1);
}

function createIosEngine() {
	return new BrowserPlaybackEngine({
		createMedia: () => media,
		isIos: true,
		mediaSession: null,
	});
}

function createMediaSessionRecorder() {
	const handlers = new Map<string, MediaSessionActionHandler | null>();
	return {
		handlers,
		session: {
			metadata: null as MediaMetadata | null,
			playbackState: "none" as MediaSessionPlaybackState,
			setActionHandler: vi.fn(
				(action: string, handler: MediaSessionActionHandler | null) => {
					handlers.set(action, handler);
				},
			),
		},
	};
}

beforeEach(() => {
	media = new MemoryMedia();
});

afterEach(() => {
	vi.unstubAllGlobals();
	vi.useRealTimers();
	vi.restoreAllMocks();
});

describe("unlockAudio", () => {
	it("tries to play the empty element so a click counts for later", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: null,
		});
		media.play.mockRejectedValueOnce(new Error("no source"));

		engine.unlockAudio();
		await Promise.resolve();

		expect(media.play).toHaveBeenCalledOnce();
		expect(engine.getState().error).toBeNull();
	});

	it("leaves a loaded source alone", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: null,
		});
		await engine.play(trackSource);
		media.play.mockClear();

		engine.unlockAudio();

		expect(media.play).not.toHaveBeenCalled();
	});
});

playbackEngineContract("browser", () => {
	const engine = new BrowserPlaybackEngine({ createMedia: () => media });
	return {
		engine,
		finish: () => media.dispatchEvent(new Event("ended")),
	};
});

playbackEngineContract("browser on iOS", () => {
	const engine = new BrowserPlaybackEngine({
		createMedia: () => media,
		isIos: true,
		mediaSession: null,
	});
	return { engine, finish: wrapLoop };
});

describe("BrowserPlaybackEngine", () => {
	it("drives the Media Session: metadata, playback state, position and actions", async () => {
		const handlers = new Map<string, MediaSessionActionHandler | null>();
		const positions: Array<MediaPositionState | undefined> = [];
		const session = {
			metadata: null as MediaMetadata | null,
			playbackState: "none" as MediaSessionPlaybackState,
			setActionHandler: vi.fn(
				(action: string, handler: MediaSessionActionHandler | null) => {
					handlers.set(action, handler);
				},
			),
			setPositionState: vi.fn((state?: MediaPositionState) => {
				positions.push(state);
			}),
		};
		vi.stubGlobal(
			"MediaMetadata",
			class {
				constructor(public init: MediaMetadataInit) {}
			},
		);
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: session,
		});

		await engine.play({ ...trackSource, artworkUrl: "/cover/album-1" });
		expect(session.playbackState).toBe("playing");
		expect(
			(session.metadata as unknown as { init: MediaMetadataInit }).init,
		).toMatchObject({
			title: "Track 1",
			artist: "Artist",
			artwork: [{ src: "/cover/album-1" }],
		});
		expect(positions.at(-1)).toMatchObject({ duration: 120, position: 0 });

		handlers.get("seekto")?.({ action: "seekto", seekTime: 42 });
		expect(engine.getState().currentTime).toBe(42);
		handlers.get("seekforward")?.({ action: "seekforward" });
		expect(engine.getState().currentTime).toBe(52);
		handlers.get("pause")?.({ action: "pause" });
		expect(engine.getState().status).toBe("paused");
		expect(session.playbackState).toBe("paused");

		const navigations: string[] = [];
		engine.subscribeNavigation((direction) => navigations.push(direction));
		handlers.get("nexttrack")?.({ action: "nexttrack" });
		handlers.get("previoustrack")?.({ action: "previoustrack" });
		expect(navigations).toEqual(["next", "previous"]);

		handlers.get("stop")?.({ action: "stop" });
		expect(engine.getState().source).toBeNull();
		expect(session.metadata).toBeNull();
		expect(session.playbackState).toBe("none");
		expect(positions.at(-1)).toBeUndefined();

		engine.destroy();
		expect(handlers.get("play")).toBeNull();
		expect(session.metadata).toBeNull();
		vi.unstubAllGlobals();
	});

	it.each(["off", "once", "loop"] as const)(
		"consumes stop-after-current without queue advancement or repetition in %s repeat mode",
		async (repeatMode) => {
			const engine = new BrowserPlaybackEngine({ createMedia: () => media });
			await engine.play(trackSource);
			while (engine.getState().repeatMode !== repeatMode) {
				engine.cycleRepeatMode();
			}
			engine.setStopAfterCurrent(true);
			const listener = vi.fn();
			engine.subscribe(listener);
			media.currentTime = 120;
			media.dispatchEvent(new Event("timeupdate"));
			listener.mockClear();
			media.paused = true;

			media.dispatchEvent(new Event("ended"));

			expect(listener).toHaveBeenCalledExactlyOnceWith(
				expect.objectContaining({
					status: "paused",
					stopAfterCurrent: false,
					source: trackSource,
					currentTime: 120,
					repeatMode,
				}),
			);
			expect(media.play).toHaveBeenCalledOnce();
			engine.destroy();
		},
	);

	it.each(["once", "loop"] as const)(
		"ends an External Track instead of repeating it in %s repeat mode, keeping the mode",
		async (repeatMode) => {
			const engine = new BrowserPlaybackEngine({ createMedia: () => media });
			while (engine.getState().repeatMode !== repeatMode) {
				engine.cycleRepeatMode();
			}
			const streamUrl = "/api/v1/external/tracks/external-1/stream";
			await engine.play({
				type: "external-track",
				externalTrack: {
					id: "external-1",
					title: "External Song",
					artistName: null,
					durationMs: 30000,
					webpageUrl: "https://soundcloud.com/uploader/external-song",
					extractor: "Soundcloud",
					hasArtwork: false,
				},
				playbackUrl: streamUrl,
				sourceUrl: streamUrl,
			});
			media.paused = true;

			media.dispatchEvent(new Event("ended"));

			expect(engine.getState()).toMatchObject({ status: "ended", repeatMode });
			expect(media.play).toHaveBeenCalledOnce();
			engine.destroy();
		},
	);

	it("publishes the buffered range end on progress events", async () => {
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(trackSource);
		expect(engine.getState().bufferedEnd).toBeNull();

		media.buffered = {
			length: 2,
			start: (index: number) => [0, 80][index] ?? 0,
			end: (index: number) => [30, 100][index] ?? 0,
		};
		media.currentTime = 10;
		media.dispatchEvent(new Event("timeupdate"));
		media.dispatchEvent(new Event("progress"));
		expect(engine.getState().bufferedEnd).toBe(30);

		media.currentTime = 50;
		media.dispatchEvent(new Event("timeupdate"));
		media.dispatchEvent(new Event("progress"));
		expect(engine.getState().bufferedEnd).toBe(100);
		engine.destroy();
	});

	it("reconnects a started Radio Station after its stream ends", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		const source = createRadioStationSource("station-reconnect");

		await engine.play(source);
		media.paused = true;
		media.dispatchEvent(new Event("ended"));

		expect(engine.getState().status).toBe("reconnecting");
		await vi.advanceTimersByTimeAsync(999);
		expect(media.play).toHaveBeenCalledOnce();
		await vi.advanceTimersByTimeAsync(1);
		expect(media.play).toHaveBeenCalledTimes(2);
		expect(engine.getState().status).toBe("playing");
		engine.destroy();
	});

	it("reconnects a started Catalog Preview after a media error", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-reconnect"));

		media.paused = true;
		media.dispatchEvent(new Event("error"));

		expect(engine.getState().status).toBe("reconnecting");
		await vi.advanceTimersByTimeAsync(1000);
		expect(media.play).toHaveBeenCalledTimes(2);
		engine.destroy();
	});

	it("keeps an initial live media failure terminal before playback is stable", async () => {
		vi.useFakeTimers();
		media.play.mockImplementationOnce(async () => {
			media.paused = false;
			media.dispatchEvent(new Event("play"));
		});
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createRadioStationSource("station-initial-failure"));

		media.dispatchEvent(new Event("error"));
		await vi.advanceTimersByTimeAsync(15000);

		expect(engine.getState().status).toBe("error");
		expect(media.play).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("reconnects when a started live stream remains stalled", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-stalled"));

		media.dispatchEvent(new Event("stalled"));
		await vi.advanceTimersByTimeAsync(9999);
		expect(engine.getState().status).toBe("playing");
		await vi.advanceTimersByTimeAsync(1);
		expect(engine.getState().status).toBe("reconnecting");
		await vi.advanceTimersByTimeAsync(1000);
		expect(media.play).toHaveBeenCalledTimes(2);
		engine.destroy();
	});

	it("cancels the stall watchdog when live playback advances", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-recovered-stall"));

		media.dispatchEvent(new Event("stalled"));
		await vi.advanceTimersByTimeAsync(5000);
		media.currentTime = 5;
		media.dispatchEvent(new Event("timeupdate"));
		await vi.advanceTimersByTimeAsync(10000);

		expect(engine.getState().status).toBe("playing");
		expect(media.play).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("resets live reconnect backoff after stable playback", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-stable"));

		media.paused = true;
		media.dispatchEvent(new Event("ended"));
		await vi.advanceTimersByTimeAsync(1000);
		expect(media.play).toHaveBeenCalledTimes(2);

		await vi.advanceTimersByTimeAsync(30000);
		media.paused = true;
		media.dispatchEvent(new Event("ended"));
		await vi.advanceTimersByTimeAsync(1000);

		expect(media.play).toHaveBeenCalledTimes(3);
		engine.destroy();
	});

	it("lets the user pause an active reconnect attempt", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-pause-reconnect"));
		media.paused = true;
		media.dispatchEvent(new Event("ended"));

		engine.togglePlay();
		await vi.advanceTimersByTimeAsync(15000);

		expect(engine.getState().status).toBe("paused");
		expect(media.play).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("does not resume when the user pauses an in-flight reconnect", async () => {
		vi.useFakeTimers();
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-in-flight-pause"));
		let resolveReconnect: () => void = () => undefined;
		media.play.mockImplementationOnce(
			() =>
				new Promise<void>((resolve) => {
					resolveReconnect = resolve;
				}),
		);
		media.dispatchEvent(new Event("ended"));

		await vi.advanceTimersByTimeAsync(1000);
		engine.pause();
		resolveReconnect();
		await Promise.resolve();
		await vi.advanceTimersByTimeAsync(30000);

		expect(engine.getState().status).toBe("paused");
		expect(media.play).toHaveBeenCalledTimes(2);
		engine.destroy();
	});

	it("caps persistent live reconnect backoff at fifteen seconds", async () => {
		vi.useFakeTimers();
		const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		await engine.play(createCatalogPreviewSource("catalog-backoff"));
		media.play.mockRejectedValue(
			new Error(
				"stream unavailable at https://secret.example/live?token=top-secret",
			),
		);
		media.paused = true;
		media.dispatchEvent(new Event("ended"));

		for (const delay of [1000, 2000, 4000, 8000, 15000, 15000]) {
			await vi.advanceTimersByTimeAsync(delay);
		}

		expect(media.play).toHaveBeenCalledTimes(7);
		expect(engine.getState().status).toBe("reconnecting");
		expect(warn).toHaveBeenCalledWith(
			"Live playback reconnect attempt failed",
			{
				sourceType: "catalog-preview",
				attempt: expect.any(Number),
				errorType: "Error",
				errorMessage: "stream unavailable at [redacted-url]",
			},
		);
		engine.destroy();
	});

	it("publishes Previous and Next through the browser queue fallback seam", () => {
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });
		const listener = vi.fn();
		engine.subscribeNavigation(listener);

		engine.previous();
		engine.next();

		expect(listener).toHaveBeenNthCalledWith(1, "previous");
		expect(listener).toHaveBeenNthCalledWith(2, "next");
		engine.destroy();
	});

	it("uses hls.js for proxied HLS Radio Stations", async () => {
		const hls = {
			attachMedia: vi.fn(),
			destroy: vi.fn(),
			loadSource: vi.fn(),
		};
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			hls: {
				isSupported: () => true,
				create: () => hls,
			},
		});

		await engine.play({
			type: "radio-station",
			station: {
				id: "station-hls",
				name: "HLS Station",
				streamUrl: "https://example.com/live/index.m3u8",
				tags: [],
				source: "manual",
				isFavorite: false,
				position: 0,
			},
			playbackUrl: "/api/v1/radio/stations/station-hls/stream",
			sourceUrl: "https://example.com/live/index.m3u8",
		});

		expect(hls.loadSource).toHaveBeenCalledWith(
			"/api/v1/radio/stations/station-hls/stream",
		);
		expect(hls.attachMedia).toHaveBeenCalledWith(media);
		expect(media.src).toBe("");
		engine.destroy();
	});

	it("reconnects a started HLS stream after a fatal HLS error", async () => {
		vi.useFakeTimers();
		let reportFatalError: () => void = () => undefined;
		const hls = {
			attachMedia: vi.fn(),
			destroy: vi.fn(),
			loadSource: vi.fn(),
			onFatalError: vi.fn((listener: () => void) => {
				reportFatalError = listener;
			}),
		};
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			hls: {
				isSupported: () => true,
				create: () => hls,
			},
		});
		await engine.play(
			createRadioStationSource(
				"station-fatal-hls",
				"https://example.com/live/index.m3u8",
			),
		);

		reportFatalError();

		expect(engine.getState().status).toBe("reconnecting");
		await vi.advanceTimersByTimeAsync(1000);
		expect(media.play).toHaveBeenCalledTimes(2);
		engine.destroy();
	});

	it("plays an HLS External Track through hls.js and ends instead of reconnecting", async () => {
		vi.useFakeTimers();
		const hls = {
			attachMedia: vi.fn(),
			destroy: vi.fn(),
			loadSource: vi.fn(),
		};
		const session = {
			metadata: null as MediaMetadata | null,
			playbackState: "none" as MediaSessionPlaybackState,
			setActionHandler: vi.fn(),
			setPositionState: vi.fn(),
		};
		vi.stubGlobal(
			"MediaMetadata",
			class {
				constructor(public init: MediaMetadataInit) {}
			},
		);
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: session,
			hls: {
				isSupported: () => true,
				create: () => hls,
			},
		});
		const streamUrl = "/api/v1/external/tracks/external-1/playlist.m3u8";

		await engine.play({
			type: "external-track",
			externalTrack: {
				id: "external-1",
				title: "External Song",
				artistName: null,
				durationMs: 30500,
				webpageUrl: "https://soundcloud.com/uploader/external-song",
				extractor: "Soundcloud",
				hasArtwork: true,
			},
			playbackUrl: streamUrl,
			sourceUrl: streamUrl,
			artworkUrl: "/api/v1/external/tracks/external-1/artwork",
		});

		expect(hls.loadSource).toHaveBeenCalledWith(streamUrl);
		expect(engine.getState().duration).toBe(30.5);
		expect(
			(session.metadata as unknown as { init: MediaMetadataInit }).init,
		).toMatchObject({
			title: "External Song",
			artist: "",
			artwork: [{ src: "/api/v1/external/tracks/external-1/artwork" }],
		});

		media.paused = true;
		media.dispatchEvent(new Event("ended"));
		await vi.advanceTimersByTimeAsync(2000);

		expect(engine.getState().status).toBe("ended");
		expect(media.play).toHaveBeenCalledOnce();
		engine.destroy();
	});

	it("does not load hls.js for non-HLS playback", async () => {
		const loadHls = vi.fn();
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			loadHls,
		});

		await engine.play({
			type: "catalog-preview",
			result: {
				stationUuid: "catalog-mp3",
				name: "MP3 Catalog",
				streamUrl: "https://example.com/live.mp3",
				tags: [],
			},
			playbackUrl: "/api/v1/radio/preview/catalog-mp3/stream",
			sourceUrl: "https://example.com/live.mp3",
		});

		expect(loadHls).not.toHaveBeenCalled();
		engine.destroy();
	});

	it("publishes playback errors without exposing media errors", async () => {
		media.play.mockRejectedValueOnce(new Error("codec unavailable"));
		const engine = new BrowserPlaybackEngine({ createMedia: () => media });

		await expect(
			engine.play({
				type: "catalog-preview",
				result: {
					stationUuid: "catalog-1",
					name: "Catalog",
					streamUrl: "https://example.com/live",
					tags: [],
				},
				playbackUrl: "/api/v1/radio/preview/catalog-1/stream",
				sourceUrl: "https://example.com/live",
			}),
		).rejects.toThrow("codec unavailable");
		expect(engine.getState()).toMatchObject({
			status: "error",
			error: {
				code: "playback-failed",
				message: "codec unavailable",
			},
		});
		engine.destroy();
	});

	it("starts a Track source at its start position, before play", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: null,
		});
		// The playhead the element already had when play() was asked for.
		let currentTimeAtPlay: number | null = null;
		const originalPlay = media.play.getMockImplementation();
		media.play.mockImplementationOnce(async () => {
			currentTimeAtPlay = media.currentTime;
			return originalPlay?.();
		});

		await engine.play({ ...trackSource, startPositionSeconds: 42 });

		expect(currentTimeAtPlay).toBe(42);
		expect(media.currentTime).toBe(42);
		expect(engine.getState().currentTime).toBe(42);
		expect(engine.getState().status).toBe("playing");
		engine.destroy();
	});

	it("fails a blocked start, naming the block, and a click starts it", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: null,
		});
		media.play.mockRejectedValueOnce(
			new DOMException("Needs a gesture", "NotAllowedError"),
		);

		await expect(engine.play(trackSource)).rejects.toThrow("Needs a gesture");

		expect(engine.getState()).toMatchObject({
			status: "error",
			error: { code: "playback-blocked" },
			source: trackSource,
		});

		engine.togglePlay();

		await vi.waitFor(() =>
			expect(engine.getState()).toMatchObject({
				status: "playing",
				error: null,
			}),
		);
		engine.destroy();
	});

	it("shows no error when a newer source interrupts a pending start", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			mediaSession: null,
		});
		let abortFirstStart: (reason: unknown) => void = () => undefined;
		media.play.mockImplementationOnce(
			() =>
				new Promise<void>((_, reject) => {
					abortFirstStart = reject;
				}),
		);
		const firstStart = engine.play(trackSource);
		await vi.waitFor(() => expect(media.play).toHaveBeenCalledOnce());

		let finishSecondStart = () => {};
		media.play.mockImplementationOnce(
			() =>
				new Promise<void>((resolve) => {
					finishSecondStart = resolve;
				}),
		);
		const newerSource = { ...trackSource, queueItemId: "queue-item-newer" };
		const secondStart = engine.play(newerSource);
		await vi.waitFor(() => expect(media.play).toHaveBeenCalledTimes(2));

		// Firefox's wording; the second source is still buffering at this point.
		abortFirstStart(
			new DOMException(
				"The fetching process for the media resource was aborted by the user agent at the user's request.",
				"AbortError",
			),
		);
		await expect(firstStart).resolves.toBeUndefined();
		media.dispatchEvent(new Event("play"));

		expect(engine.getState()).toMatchObject({
			status: "playing",
			error: null,
			source: newerSource,
		});
		finishSecondStart();
		await secondStart;
		engine.destroy();
	});
});

describe("BrowserPlaybackEngine on other browsers", () => {
	it("keeps all eight Media Session actions without re-registering them", async () => {
		const { handlers, session } = createMediaSessionRecorder();
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			isIos: false,
			mediaSession: session,
		});

		await engine.play(trackSource);

		expect(session.setActionHandler).toHaveBeenCalledTimes(8);
		expect([...handlers.values()].every(Boolean)).toBe(true);
		engine.destroy();
	});

	it("tears the source down between Tracks and never loops", async () => {
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			isIos: false,
			mediaSession: null,
		});

		await engine.play(trackSource);
		wrapLoop();

		expect(media.removeAttribute).toHaveBeenCalledWith("src");
		expect(media.load).toHaveBeenCalledOnce();
		expect(media.loop).toBe(false);
		expect(engine.getState()).toMatchObject({
			status: "playing",
			currentTime: 0.1,
		});
		engine.destroy();
	});

	it("still prefers hls.js when native HLS is available too", async () => {
		media.canPlayType.mockReturnValue("maybe");
		const hls = {
			attachMedia: vi.fn(),
			destroy: vi.fn(),
			loadSource: vi.fn(),
		};
		const engine = new BrowserPlaybackEngine({
			createMedia: () => media,
			hls: { isSupported: () => true, create: () => hls },
			isIos: false,
		});

		await engine.play(
			createRadioStationSource(
				"station-hls-js",
				"https://example.com/live/index.m3u8",
			),
		);

		expect(hls.attachMedia).toHaveBeenCalledWith(media);
		engine.destroy();
	});
});

describe("BrowserPlaybackEngine on iOS", () => {
	describe("audio session", () => {
		it.each([
			["iPhone", "playback", { platform: "iPhone", maxTouchPoints: 5 }],
			["iPadOS", "playback", { platform: "MacIntel", maxTouchPoints: 5 }],
			["a Mac", "auto", { platform: "MacIntel", maxTouchPoints: 0 }],
			["Linux", "auto", { platform: "Linux x86_64", maxTouchPoints: 0 }],
		])("detection gives %s the audio session type %s", (_, type, device) => {
			const audioSession = { type: "auto" };
			vi.stubGlobal("navigator", { ...device, audioSession });

			new BrowserPlaybackEngine({
				createMedia: () => media,
				mediaSession: null,
			}).destroy();

			expect(audioSession.type).toBe(type);
		});

		it("survives a browser whose audio session throws", () => {
			vi.stubGlobal("navigator", {
				audioSession: {
					set type(_value: string) {
						throw new Error("not allowed");
					},
				},
			});

			expect(() => createIosEngine().destroy()).not.toThrow();
		});

		it("survives a browser without one", () => {
			vi.stubGlobal("navigator", {});

			expect(() => createIosEngine().destroy()).not.toThrow();
		});
	});

	describe("lock-screen buttons", () => {
		function createEngineWithSession() {
			const recorder = createMediaSessionRecorder();
			const engine = new BrowserPlaybackEngine({
				createMedia: () => media,
				isIos: true,
				mediaSession: recorder.session,
			});
			return { ...recorder, engine };
		}

		it("clears the seek skips and keeps seekto", () => {
			const { engine, handlers } = createEngineWithSession();

			expect(handlers.get("seekbackward")).toBeNull();
			expect(handlers.get("seekforward")).toBeNull();
			handlers.get("seekto")?.({ action: "seekto", seekTime: 42 });
			expect(engine.getState().currentTime).toBe(42);
			engine.destroy();
		});

		it("re-registers the buttons on every playing event", async () => {
			const { engine, handlers, session } = createEngineWithSession();
			const registeredActions = () =>
				session.setActionHandler.mock.calls
					.filter(([, handler]) => handler)
					.map(([action]) => action);
			await engine.play(trackSource);
			session.setActionHandler.mockClear();

			media.dispatchEvent(new Event("playing"));
			media.dispatchEvent(new Event("playing"));

			for (const action of ["previoustrack", "nexttrack", "play", "pause"]) {
				expect(
					registeredActions().filter((name) => name === action),
				).toHaveLength(2);
			}
			expect(handlers.get("seekbackward")).toBeNull();
			expect(handlers.get("seekforward")).toBeNull();
			engine.destroy();
		});

		it("drives Next through a re-registered handler", async () => {
			const { engine, handlers } = createEngineWithSession();
			const navigation = vi.fn();
			engine.subscribeNavigation(navigation);
			await engine.play(trackSource);

			handlers.get("nexttrack")?.({ action: "nexttrack" });

			expect(navigation).toHaveBeenCalledExactlyOnceWith("next");
			engine.destroy();
		});

		it("still cleans up, and stays clean after destroy", async () => {
			const { engine, handlers } = createEngineWithSession();
			await engine.play(trackSource);

			engine.destroy();
			media.dispatchEvent(new Event("playing"));

			expect(handlers.size).toBe(8);
			expect([...handlers.values()].some(Boolean)).toBe(false);
		});
	});

	describe("track change while locked", () => {
		it("loops only Tracks", async () => {
			const engine = createIosEngine();

			await engine.play(trackSource);
			expect(media.loop).toBe(true);
			await engine.play(createRadioStationSource("station-loop"));
			expect(media.loop).toBe(false);
			await engine.play(externalTrackSource);
			expect(media.loop).toBe(false);
			await engine.play(createCatalogPreviewSource("catalog-loop"));
			expect(media.loop).toBe(false);
			engine.destroy();
		});

		it("does not loop a Track without a known duration", async () => {
			const engine = createIosEngine();

			await engine.play({
				...trackSource,
				track: { ...trackSource.track, durationMs: 0 },
			});

			expect(media.loop).toBe(false);
			engine.destroy();
		});

		it("ends once on a wrap and keeps the element playing for the next Track", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(119.9);
			const statuses: string[] = [];
			engine.subscribe((state) => statuses.push(state.status));

			reachTime(0.1);
			reachTime(0.35);
			reachTime(0.6);

			expect(statuses).toEqual(["ended"]);
			expect(media.pause).not.toHaveBeenCalled();
			engine.destroy();
		});

		it("leaves the wrapped element playing into the next Track", async () => {
			const engine = createIosEngine();
			const nextSource = { ...trackSource, playbackUrl: "/stream/track-2" };
			await engine.play(trackSource);
			wrapLoop();
			media.removeAttribute.mockClear();

			await engine.play(nextSource);
			reachTime(0.2);

			expect(media.src).toBe("/stream/track-2");
			expect(media.pause).not.toHaveBeenCalled();
			expect(engine.getState()).toMatchObject({
				source: nextSource,
				status: "playing",
				currentTime: 0.2,
			});
			engine.destroy();
		});

		it("does not take a skip near the end for a wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(119.5);

			await engine.play({ ...trackSource, playbackUrl: "/stream/track-2" });
			reachTime(0.2);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0.2,
			});
			engine.destroy();
		});

		it("stops a replaying element when nothing follows the end", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			wrapLoop();

			reachTime(1);

			expect(media.paused).toBe(true);
			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("does not read rebuffering after a wrap as a resumed Track", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			wrapLoop();

			media.dispatchEvent(new Event("playing"));

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("stops at the end of the Queue when the Provider stops", async () => {
			const engine = createIosEngine();
			engine.subscribe((state) => {
				if (state.status === "ended") engine.stop();
			});
			await engine.play(trackSource);

			wrapLoop();
			media.currentTime = 0;
			media.dispatchEvent(new Event("timeupdate"));

			expect(media.paused).toBe(true);
			expect(media.removeAttribute).toHaveBeenCalledWith("src");
			expect(engine.getState()).toMatchObject({
				source: null,
				status: "idle",
			});
			engine.destroy();
		});

		it("does not take the position reset of stop() for a wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(119.9);

			engine.stop();
			reachTime(0);

			expect(engine.getState().status).toBe("idle");
			engine.destroy();
		});

		it("repeats once, then ends on the next wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			engine.cycleRepeatMode();

			wrapLoop();

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0,
				repeatMode: "off",
			});
			expect(media.currentTime).toBe(0);

			wrapLoop();

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("repeats on every wrap in repeat loop", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			engine.cycleRepeatMode();
			engine.cycleRepeatMode();

			wrapLoop();
			wrapLoop();

			expect(engine.getState()).toMatchObject({
				status: "playing",
				repeatMode: "loop",
			});
			expect(media.paused).toBe(false);
			engine.destroy();
		});

		it.each(["off", "once", "loop"] as const)(
			"pauses for stop-after-current without repeating in %s repeat mode",
			async (repeatMode) => {
				const engine = createIosEngine();
				await engine.play(trackSource);
				while (engine.getState().repeatMode !== repeatMode) {
					engine.cycleRepeatMode();
				}
				engine.setStopAfterCurrent(true);

				wrapLoop();

				expect(media.paused).toBe(true);
				expect(media.play).toHaveBeenCalledOnce();
				expect(engine.getState()).toMatchObject({
					status: "paused",
					stopAfterCurrent: false,
					source: trackSource,
					repeatMode,
				});
				engine.destroy();
			},
		);

		it("resumes after stop-after-current without taking the restart for a wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			engine.setStopAfterCurrent(true);
			wrapLoop();

			await engine.play();
			reachTime(0.4);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0.4,
			});
			engine.destroy();
		});

		it("resumes after an ended pause without ending again", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			wrapLoop();
			reachTime(1);

			await engine.play();
			reachTime(1.25);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 1.25,
			});
			engine.destroy();
		});

		it("lets an External Track end by itself, since nothing follows it", async () => {
			const engine = createIosEngine();
			await engine.play(externalTrackSource);

			media.dispatchEvent(new Event("ended"));

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("takes a wrap that comes well before the reported duration", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			expect(engine.getState().duration).toBe(120);

			wrapLoop(20);

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("keeps Repeat once on a Track shorter than the wrap edge", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			engine.cycleRepeatMode();

			wrapLoop(4);
			reachTime(0.05);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				repeatMode: "off",
			});
			engine.destroy();
		});

		it("does not treat a seek back to the start as a wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(119.5);

			engine.seek(0);
			reachTime(0.2);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0.2,
			});
			engine.destroy();
		});

		it("does not treat a lock-screen scrub to the start as a wrap", async () => {
			const { handlers, session } = createMediaSessionRecorder();
			const engine = new BrowserPlaybackEngine({
				createMedia: () => media,
				isIos: true,
				mediaSession: session,
			});
			await engine.play(trackSource);
			reachTime(119.5);

			handlers.get("seekto")?.({ action: "seekto", seekTime: 0 });
			reachTime(0.2);

			expect(engine.getState().status).toBe("playing");
			engine.destroy();
		});

		it("still takes a wrap after a seek to the end for one", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);

			engine.seek(119.9);
			reachTime(0.1);

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("leaves a small jump backward near the start alone", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(3);

			reachTime(2.5);

			expect(engine.getState().status).toBe("playing");
			engine.destroy();
		});

		it("still honours a natural ended if one fires", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			reachTime(120);
			media.paused = true;

			media.dispatchEvent(new Event("ended"));

			expect(engine.getState().status).toBe("ended");
			engine.destroy();
		});

		it("does not take the restart after a natural ended for a wrap", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			engine.setStopAfterCurrent(true);
			reachTime(120);
			media.paused = true;
			media.dispatchEvent(new Event("ended"));

			await engine.play();
			reachTime(0.2);

			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0.2,
			});
			engine.destroy();
		});
	});

	describe("source swap", () => {
		it("assigns src directly between Tracks", async () => {
			const engine = createIosEngine();
			await engine.play(trackSource);
			await engine.play({ ...trackSource, playbackUrl: "/stream/track-2" });

			expect(media.src).toBe("/stream/track-2");
			expect(media.removeAttribute).not.toHaveBeenCalled();
			expect(media.load).not.toHaveBeenCalled();
			engine.destroy();
		});

		it("restores the speed after assigning src, which resets it", async () => {
			let src = "";
			const rateMedia = Object.assign(media, { playbackRate: 1 });
			Object.defineProperty(rateMedia, "src", {
				get: () => src,
				set: (value: string) => {
					src = value;
					rateMedia.playbackRate = 1;
				},
			});
			const engine = createIosEngine();
			engine.setPlaybackRate(1.5);

			await engine.play(trackSource);

			expect(rateMedia.playbackRate).toBe(1.5);
			engine.destroy();
		});

		it.each(["stop", "destroy"] as const)(
			"%s() still clears it",
			async (method) => {
				const engine = createIosEngine();
				await engine.play(trackSource);

				engine[method]();

				expect(media.removeAttribute).toHaveBeenCalledWith("src");
				expect(media.load).toHaveBeenCalledOnce();
				engine.destroy();
			},
		);
	});

	describe("radio", () => {
		const hlsSource = () =>
			createRadioStationSource(
				"station-native-hls",
				"https://example.com/live/index.m3u8",
			);

		it("prefers native HLS over hls.js", async () => {
			media.canPlayType.mockReturnValue("maybe");
			const loadHls = vi.fn();
			const engine = new BrowserPlaybackEngine({
				createMedia: () => media,
				isIos: true,
				loadHls,
			});

			await engine.play(hlsSource());

			expect(media.src).toBe(hlsSource().playbackUrl);
			expect(loadHls).not.toHaveBeenCalled();
			expect(media.loop).toBe(false);
			engine.destroy();
		});

		it("falls back to hls.js without native HLS", async () => {
			const hls = {
				attachMedia: vi.fn(),
				destroy: vi.fn(),
				loadSource: vi.fn(),
			};
			const engine = new BrowserPlaybackEngine({
				createMedia: () => media,
				hls: { isSupported: () => true, create: () => hls },
				isIos: true,
			});

			await engine.play(hlsSource());

			expect(hls.attachMedia).toHaveBeenCalledWith(media);
			engine.destroy();
		});

		it("reconnects a dropped station without looping", async () => {
			vi.useFakeTimers();
			const engine = createIosEngine();
			await engine.play(createRadioStationSource("station-ios-reconnect"));

			media.paused = true;
			media.dispatchEvent(new Event("ended"));
			await vi.advanceTimersByTimeAsync(1000);

			expect(media.play).toHaveBeenCalledTimes(2);
			expect(media.loop).toBe(false);
			expect(engine.getState().status).toBe("playing");
			engine.destroy();
		});
	});
});
