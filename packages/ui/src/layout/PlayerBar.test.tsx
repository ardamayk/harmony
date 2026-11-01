import type {
	ExternalTrack,
	ListenTogetherRoom,
	RadioStation,
} from "@repo/api-client";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
	within,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	RoomPlaybackContext,
	type RoomPlayback,
} from "../listen-together/RoomPlaybackContext";
import {
	type PlaybackApi,
	PlaybackProvider,
	usePlayback,
} from "../playback/PlaybackProvider";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import { defaultPreferences } from "../widgets/types";
import { LayoutProvider } from "./LayoutProvider";
import { PlayerBar } from "./PlayerBar";

const navigate = vi.fn();

vi.mock("@tanstack/react-router", () => ({
	useNavigate: () => navigate,
	useRouter: () => null,
}));

const track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [{ id: "artist-id", name: "Artist", role: "main" }],
	albumId: "album-1",
	discNo: 1,
	albumTitle: "Album 1",
	durationMs: 120000,
	format: "flac",
	genres: [],
	sampleRateHz: 96000,
	bitDepth: 24,
	bitrateKbps: 1411,
	replayGain: {
		trackGainDb: -7.25,
		trackPeak: null,
		albumGainDb: null,
		albumPeak: null,
	},
};

const radioStation: RadioStation = {
	id: "station-1",
	name: "Radio Paradise Main Mix",
	streamUrl: "https://example.com/radio.mp3",
	faviconUrl: "https://example.com/radio.png",
	country: "United States",
	language: "English",
	tags: ["eclectic"],
	source: "manual",
	isFavorite: false,
	position: 0,
};

const api: PlaybackApi = {
	getQueue: vi.fn(async () => ({
		items: [{ id: "item-1", trackId: track.id, position: 0, track }],
		revision: "1",
	})),
	replaceQueue: vi.fn(async () => ({
		items: [{ id: "item-1", trackId: track.id, position: 0, track }],
		revision: "2",
	})),
	reorderQueue: vi.fn(async () => ({
		items: [{ id: "item-1", trackId: track.id, position: 0, track }],
		revision: "2",
	})),
	appendQueueItem: vi.fn(async () => ({
		items: [{ id: "item-1", trackId: track.id, position: 0, track }],
		revision: "2",
	})),
	removeQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
	clearQueue: vi.fn(async () => ({ items: [], revision: "2" })),
	getStreamUrl: (trackId) => `/stream/${trackId}`,
	getAlbumCoverUrl: (albumId) => `/cover/${albumId}`,
	getTrackLyrics: vi.fn(async () => ({ lyrics: "Line one\nLine two" })),
	getRadioStationStreamUrl: (stationId) => `/radio/${stationId}`,
	getRadioCatalogPreviewStreamUrl: (stationUuid) =>
		`/radio/preview/${stationUuid}`,
	getRadioNowPlaying: vi.fn(async () => ({})),
	getExternalTrackPlayback: vi.fn(async (externalTrackId: string) => ({
		format: "progressive" as const,
		streamPath: `/api/v1/external/tracks/${externalTrackId}/stream`,
	})),
	getExternalStreamUrl: (streamPath: string) => `/server${streamPath}`,
	getExternalTrackArtworkUrl: (externalTrackId: string) =>
		`/server/api/v1/external/tracks/${externalTrackId}/artwork`,
	listPlaylists: vi.fn(async () => ({
		items: [
			{ id: "favorites", name: "Favorites", isDefault: true, trackCount: 0 },
			{ id: "road", name: "Road", isDefault: false, trackCount: 1 },
		],
		total: 2,
	})),
	getPlaylist: vi.fn(async (playlistId: string) => ({
		id: playlistId,
		name: playlistId === "favorites" ? "Favorites" : "Road",
		isDefault: playlistId === "favorites",
		trackCount: playlistId === "favorites" ? 1 : 0,
		tracks: playlistId === "favorites" ? [track] : [],
	})),
	createPlaylist: vi.fn(async (name) => ({
		id: "new",
		name,
		isDefault: false,
		trackCount: 0,
	})),
	addPlaylistTrack: vi.fn(async (playlistId) => ({
		id: playlistId,
		name: playlistId === "favorites" ? "Favorites" : "Road",
		isDefault: playlistId === "favorites",
		trackCount: 1,
		tracks: [track],
	})),
	removePlaylistTrack: vi.fn(async (playlistId) => ({
		id: playlistId,
		name: playlistId === "favorites" ? "Favorites" : "Road",
		isDefault: playlistId === "favorites",
		trackCount: 0,
		tracks: [],
	})),
};

function PlaybackStarter() {
	const playback = usePlayback();
	return (
		<button type="button" onClick={() => void playback.playTrack(track.id)}>
			Start track
		</button>
	);
}

function RadioStarter() {
	const playback = usePlayback();
	return (
		<button
			type="button"
			onClick={() => void playback.playRadioStation(radioStation)}
		>
			Start radio
		</button>
	);
}

function renderPlayerBar(
	onPlaylistMutated?: () => void,
	engine = new InMemoryPlaybackEngine(),
	props: Partial<Parameters<typeof PlayerBar>[0]> = {},
) {
	const result = render(
		<LayoutProvider initialPreferences={defaultPreferences}>
			<PlaybackProvider api={api} engine={engine}>
				<PlaybackStarter />
				<RadioStarter />
				<PlayerBar onPlaylistMutated={onPlaylistMutated} {...props} />
			</PlaybackProvider>
		</LayoutProvider>,
	);
	return { ...result, engine };
}

async function openActionsMenu() {
	await act(async () => {
		screen.getByRole("button", { name: "Start track" }).click();
	});
	fireEvent.click(screen.getByRole("button", { name: "Track actions" }));
}

async function openPlaylistSubmenu() {
	await openActionsMenu();
	fireEvent.mouseEnter(
		screen.getByRole("menuitem", { name: "Add to playlist" }),
	);
}

describe("PlayerBar", () => {
	beforeEach(() => {
		navigate.mockClear();
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
		vi.restoreAllMocks();
	});

	it("renders an empty disabled playback state", () => {
		renderPlayerBar();

		expect(screen.getByText("Nothing playing")).toBeTruthy();
		expect(screen.getByText("Select a track")).toBeTruthy();
		expect(
			(screen.getByRole("button", { name: "Play" }) as HTMLButtonElement)
				.disabled,
		).toBe(true);
		// Nothing is playing: the quality pill is a plain status, not a menu.
		expect(screen.queryByRole("button", { name: /^Quality/ })).toBeNull();
		expect(screen.getByLabelText("Quality Quality")).toBeTruthy();
	});

	it("shows the current track after playback starts", async () => {
		renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		expect(screen.getByText("Track 1")).toBeTruthy();
		expect(screen.getByText("Artist")).toBeTruthy();
		expect(screen.getByText("Album 1")).toBeTruthy();
	});

	it("shows an active reconnecting state for live radio", async () => {
		const { engine } = renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start radio" }).click();
		});
		act(() => engine.reconnect());

		expect(screen.getByRole("status").textContent).toBe("Reconnecting…");
		expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
	});

	it("keeps the now-playing region at a stable width independent of title length", async () => {
		renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		const nowPlaying = screen.getByLabelText("Now playing");
		expect(nowPlaying.className).toContain("min-w-50");
		expect(nowPlaying.className).toContain("grow shrink-0 basis-0");
	});

	it("keeps playback controls centered in the full player bar", async () => {
		renderPlayerBar();

		const controls = screen.getByLabelText("Playback controls");
		expect(controls.parentElement?.className).toContain("justify-between");
		expect(controls.className).toContain("justify-self-center");
	});

	it("renders the Figma player bar shell dimensions", () => {
		renderPlayerBar();

		expect(screen.getByRole("contentinfo").className).toContain("h-21.5");
		expect(screen.getByRole("contentinfo").className).toContain("bg-player");
	});

	it("toggles the current Track's favorite state from the Player Bar", async () => {
		const onToggleFavorite = vi.fn();
		render(
			<LayoutProvider initialPreferences={defaultPreferences}>
				<PlaybackProvider api={api} engine={new InMemoryPlaybackEngine()}>
					<PlaybackStarter />
					<PlayerBar
						isCurrentTrackFavorite={false}
						onToggleFavorite={onToggleFavorite}
					/>
				</PlaybackProvider>
			</LayoutProvider>,
		);
		expect(
			(
				screen.getByRole("button", {
					name: "Add to favorites",
				}) as HTMLButtonElement
			).disabled,
		).toBe(true);

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));
		expect(onToggleFavorite).toHaveBeenCalledWith(track.id);
	});

	it("renders a round primary play control and a thin seek bar", async () => {
		renderPlayerBar();
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		expect(screen.getByRole("button", { name: "Pause" }).className).toContain(
			"rounded-full",
		);
		const seek = screen.getByLabelText("Seek") as HTMLInputElement;
		expect(seek.className).toContain("player-seek-slider");
		expect(seek.style.getPropertyValue("--seek-level")).toMatch(/%$/);
	});

	it("opens the full-screen Lyrics view with the Queue beside it", async () => {
		renderPlayerBar();
		expect(
			(screen.getByRole("button", { name: "Lyrics" }) as HTMLButtonElement)
				.disabled,
		).toBe(true);

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));

		const view = screen.getByRole("dialog", { name: "Lyrics" });
		expect(await within(view).findByText("Line one")).toBeTruthy();
		expect(within(view).getByText("Line two")).toBeTruthy();
		expect(api.getTrackLyrics).toHaveBeenCalledWith(track.id);
		expect(within(view).getByRole("region", { name: "Queue" })).toBeTruthy();
		expect(within(view).getAllByText("Track 1").length).toBeGreaterThan(0);

		fireEvent.keyDown(document, { key: "Escape" });
		expect(screen.queryByRole("dialog", { name: "Lyrics" })).toBeNull();
	});

	it("mutes from the volume icon and restores the previous level on unmute", async () => {
		renderPlayerBar();

		fireEvent.change(screen.getByLabelText("Volume"), {
			target: { value: "0.4" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Mute" }));
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0",
		);

		fireEvent.click(screen.getByRole("button", { name: "Unmute" }));
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0.4",
		);
	});

	it("keeps volume slider usable", async () => {
		renderPlayerBar();

		fireEvent.change(screen.getByLabelText("Volume"), {
			target: { value: "0.3" },
		});
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0.3",
		);

		fireEvent.change(screen.getByLabelText("Volume"), {
			target: { value: "0" },
		});
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0",
		);
	});

	it("toggles the active Playback Session from the primary control", async () => {
		renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		fireEvent.click(screen.getByRole("button", { name: "Pause" }));

		expect(screen.getByRole("button", { name: "Play" })).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
	});

	it("renders bit depth and sample rate quality details with track actions menu", async () => {
		renderPlayerBar();
		await openActionsMenu();

		expect(screen.getByLabelText("Quality 24-bit · 96 kHz")).toBeTruthy();
		expect(screen.getByText("24-bit · 96 kHz")).toBeTruthy();
		expect(screen.getByText("Add to playlist")).toBeTruthy();
		expect(screen.getByRole("menuitem", { name: "Play next" })).toBeTruthy();
		expect(screen.getByText("Go to album")).toBeTruthy();
		expect(screen.getByText("Go to artist")).toBeTruthy();
		expect(
			(screen.getByRole("menuitem", { name: "Download" }) as HTMLButtonElement)
				.disabled,
		).toBe(true);
		expect(screen.getByRole("menuitem", { name: "Details" })).toBeTruthy();
	});

	it("shows a dash for tracks that carry no bit depth", async () => {
		const bitDepth = track.bitDepth;
		track.bitDepth = 0;
		try {
			renderPlayerBar();
			await openActionsMenu();

			expect(screen.getByLabelText("Quality - · 96 kHz")).toBeTruthy();
		} finally {
			track.bitDepth = bitDepth;
		}
	});

	it("opens compact output modes in the Player Bar and applies Normal", async () => {
		const engine = new InMemoryPlaybackEngine({ outputMode: "direct-alsa" });
		const fallbackToSystemOutput = vi.spyOn(engine, "fallbackToSystemOutput");
		renderPlayerBar(undefined, engine);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		const trigger = screen.getByRole("button", {
			name: "Quality 24-bit · 96 kHz",
		});
		expect(trigger.getAttribute("title")).toBe("Output mode: Exclusive");
		fireEvent.click(trigger);
		const menu = screen.getByRole("menu", { name: "Output mode" });
		expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(3);
		fireEvent.click(
			within(menu).getByRole("menuitemradio", { name: "Normal" }),
		);

		expect(fallbackToSystemOutput).toHaveBeenCalledOnce();
		expect(
			screen
				.getByRole("button", { name: "Quality 24-bit · 96 kHz" })
				.getAttribute("title"),
		).toBe("Output mode: Normal");
	});

	it("shows live radio progress instead of track time", async () => {
		renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start radio" }).click();
		});

		expect(screen.getByText("LIVE")).toBeTruthy();
		expect(screen.queryByText("--:--")).toBeNull();
		expect(screen.queryByText("0:00")).toBeNull();
		expect(screen.queryByLabelText("Seek")).toBeNull();
		expect(screen.getByLabelText("Quality High Quality")).toBeTruthy();
	});

	it("says where the sound is while following another device", () => {
		renderPlayerBar(
			undefined,
			new InMemoryPlaybackEngine({
				sharedPlayback: { followingDeviceName: "Harmony on desktop" },
			}),
		);

		expect(screen.getByText("Playing on Harmony on desktop")).toBeTruthy();
	});

	it("names the External Source next to the External label", async () => {
		const { engine } = renderPlayerBar(undefined, undefined, {
			externalSourceName: "SoundCloud",
		});
		await act(async () => {
			await engine.play({
				type: "external-track",
				externalTrack: {
					id: "external-2",
					title: "Named Song",
					artistName: "Artist",
					durationMs: 90000,
					webpageUrl: null,
					extractor: "Soundcloud",
					hasArtwork: false,
				},
				playbackUrl: "/api/v1/external/tracks/external-2/stream",
				sourceUrl: "/api/v1/external/tracks/external-2/stream",
			});
		});

		expect(screen.getByText("External – SoundCloud")).toBeTruthy();
	});

	it("shows an External Track with a label, seeking only, no favorite and no track actions", async () => {
		vi.useFakeTimers();
		try {
			// The host wires favorites; an External Track still offers none.
			const { engine } = renderPlayerBar(undefined, undefined, {
				onToggleFavorite: vi.fn(),
				onToggleStationFavorite: vi.fn(),
			});
			const externalTrack: ExternalTrack = {
				id: "external-1",
				title: "External Song",
				artistName: null,
				durationMs: 90000,
				webpageUrl: null,
				extractor: "Soundcloud",
				hasArtwork: true,
			};
			await act(async () => {
				await engine.play({
					type: "external-track",
					externalTrack,
					playbackUrl: "/api/v1/external/tracks/external-1/stream",
					sourceUrl: "/api/v1/external/tracks/external-1/stream",
				});
			});
			act(() => {
				vi.advanceTimersByTime(500);
			});

			expect(screen.getByText("External Song")).toBeTruthy();
			expect(screen.getByText("Soundcloud")).toBeTruthy();
			expect(screen.getByText("External")).toBeTruthy();
			// Its cover, through the Music Server, like an Album cover.
			expect(
				screen
					.getByRole("region", { name: "Now playing" })
					.querySelector("img")
					?.getAttribute("src"),
			).toBe("/server/api/v1/external/tracks/external-1/artwork");
			expect(screen.getByText("1:30")).toBeTruthy();
			expect(screen.queryByText("LIVE")).toBeNull();
			expect((screen.getByLabelText("Seek") as HTMLInputElement).disabled).toBe(
				false,
			);
			// Not in the library and not in the Queue: no favorite, no track
			// actions, no Queue controls; only Play/Pause and seeking.
			for (const name of [
				"Add to favorites",
				"Remove from favorites",
				"Track actions",
				"Shuffle off",
				"Shuffle on",
				"Previous",
				"Next",
				"Repeat off",
				"Repeat once",
				"Repeat loop",
			]) {
				expect(screen.queryByRole("button", { name })).toBeNull();
			}
			expect(
				(screen.getByRole("button", { name: "Pause" }) as HTMLButtonElement)
					.disabled,
			).toBe(false);
			expect(screen.getByTestId("now-playing-announcer").textContent).toBe(
				"Now playing: External Song",
			);
		} finally {
			vi.useRealTimers();
		}
	});

	it("shows an inline error banner with Retry and Skip after a failed Track", async () => {
		const { engine } = renderPlayerBar();
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		await act(async () =>
			engine.fail({ code: "playback-failed", message: "Playback failed" }),
		);
		await act(async () => {});

		const banner = screen.getByTestId("playback-error-banner");
		expect(banner.textContent).toContain("Playback failed");
		expect(within(banner).getByRole("button", { name: "Retry" })).toBeTruthy();
		// The test queue holds one item, so there is nothing to skip to.
		expect(within(banner).queryByRole("button", { name: "Skip" })).toBeNull();

		const play = vi.spyOn(engine, "play");
		fireEvent.click(within(banner).getByRole("button", { name: "Retry" }));
		expect(play).toHaveBeenCalledOnce();
	});

	it("changes speed, arms the sleep timer and queues the Track from the actions menu", async () => {
		const { engine } = renderPlayerBar();
		const setPlaybackRate = vi.spyOn(engine, "setPlaybackRate");
		await openActionsMenu();

		fireEvent.click(screen.getByRole("menuitemradio", { name: "1.5×" }));
		expect(setPlaybackRate).toHaveBeenLastCalledWith(1.5);

		fireEvent.click(screen.getByRole("menuitemradio", { name: "After track" }));
		expect(engine.getState().stopAfterCurrent).toBe(true);

		fireEvent.click(screen.getByRole("menuitem", { name: "Add to queue" }));
		await act(async () => {});
		expect(api.appendQueueItem).toHaveBeenCalledWith(
			track.id,
			expect.any(String),
		);
	});

	it("follows synced lyrics and seeks to a line when it is clicked", async () => {
		const { engine } = renderPlayerBar();
		vi.mocked(api.getTrackLyrics)?.mockResolvedValueOnce({
			lyrics: "[00:10.00] First line\n[00:20.00] Second line",
		});
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		const second = await screen.findByRole("button", { name: "Second line" });
		await act(async () => engine.seek(12));
		expect(
			screen
				.getByRole("button", { name: "First line" })
				.getAttribute("aria-current"),
		).toBe("true");

		const seek = vi.spyOn(engine, "seek");
		fireEvent.click(second);
		expect(seek).toHaveBeenLastCalledWith(20);
	});

	it("shows the host's window buttons in the Lyrics view", async () => {
		const windowControls = {
			minimize: vi.fn(),
			toggleMaximize: vi.fn(),
			close: vi.fn(),
		};
		render(
			<LayoutProvider initialPreferences={defaultPreferences}>
				<PlaybackProvider api={api} engine={new InMemoryPlaybackEngine()}>
					<PlaybackStarter />
					<PlayerBar windowControls={windowControls} />
				</PlaybackProvider>
			</LayoutProvider>,
		);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		const view = screen.getByRole("dialog", { name: "Lyrics" });
		fireEvent.click(within(view).getByRole("button", { name: "Minimize" }));
		expect(windowControls.minimize).toHaveBeenCalled();
	});

	it("keeps empty lyrics settled while the playhead advances", async () => {
		const { engine } = renderPlayerBar();
		vi.mocked(api.getTrackLyrics)?.mockResolvedValueOnce({ lyrics: "" });
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		vi.mocked(api.getTrackLyrics)?.mockClear();
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		expect(await screen.findByText("No lyrics yet")).toBeTruthy();
		await act(async () => engine.seek(10));
		await act(async () => engine.seek(11));
		expect(api.getTrackLyrics).toHaveBeenCalledTimes(1);
		expect(screen.getByText("No lyrics yet")).toBeTruthy();
	});

	it("keeps loaded lyrics while playback updates without another request", async () => {
		const { engine } = renderPlayerBar();
		await act(async () =>
			screen.getByRole("button", { name: "Start track" }).click(),
		);
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		expect(await screen.findByText("Line one")).toBeTruthy();
		await act(async () => engine.seek(12));
		await act(async () => engine.pause());
		expect(api.getTrackLyrics).toHaveBeenCalledTimes(1);
		expect(screen.getByText("Line two")).toBeTruthy();
		expect(screen.queryByText("Loading lyrics…")).toBeNull();
	});

	it("keeps a lyrics failure settled until the view is reopened", async () => {
		const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
		vi.mocked(api.getTrackLyrics)?.mockRejectedValueOnce(
			new Error("Server unavailable"),
		);
		const { engine } = renderPlayerBar();
		await act(async () =>
			screen.getByRole("button", { name: "Start track" }).click(),
		);
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		expect(await screen.findByRole("alert")).toBeTruthy();
		await act(async () => engine.seek(12));
		expect(api.getTrackLyrics).toHaveBeenCalledTimes(1);
		expect(screen.getByText("Lyrics could not be loaded")).toBeTruthy();
		expect(warning).toHaveBeenCalledWith(
			"Failed to load track lyrics",
			expect.objectContaining({ trackId: track.id }),
		);
		fireEvent.click(screen.getByRole("button", { name: "Close lyrics" }));
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		expect(await screen.findByText("Line one")).toBeTruthy();
		expect(api.getTrackLyrics).toHaveBeenCalledTimes(2);
	});

	it("ignores old lyrics that arrive after the track changes", async () => {
		let finishOldLyrics = (_result: { lyrics: string }) => {};
		vi.mocked(api.getTrackLyrics)?.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					finishOldLyrics = resolve;
				}),
		);
		vi.mocked(api.getTrackLyrics)?.mockResolvedValueOnce({
			lyrics: "Current track lyrics",
		});
		const { engine } = renderPlayerBar();
		await act(async () =>
			screen.getByRole("button", { name: "Start track" }).click(),
		);
		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));
		expect(screen.getByText("Loading lyrics…")).toBeTruthy();
		await act(async () =>
			engine.play({
				type: "track",
				track: { ...track, id: "track-2", title: "Track 2" },
				playbackUrl: "/stream/track-2",
			}),
		);
		expect(await screen.findByText("Current track lyrics")).toBeTruthy();
		await act(async () => finishOldLyrics({ lyrics: "Stale lyrics" }));
		expect(screen.queryByText("Stale lyrics")).toBeNull();
		expect(screen.getByText("Current track lyrics")).toBeTruthy();
		expect(api.getTrackLyrics).toHaveBeenNthCalledWith(2, "track-2");
		expect(api.getTrackLyrics).toHaveBeenCalledTimes(2);
	});

	it("shows gapless as an icon before quality only for an active source", async () => {
		const { engine } = renderPlayerBar(
			undefined,
			new InMemoryPlaybackEngine({ isGapless: true }),
		);
		expect(
			screen.queryByRole("img", { name: "Gapless playback enabled" }),
		).toBeNull();
		await act(async () =>
			screen.getByRole("button", { name: "Start track" }).click(),
		);
		const icon = screen.getByRole("img", { name: "Gapless playback enabled" });
		expect(icon.textContent).toBe("");
		const quality = screen.getByRole("note", { name: /^Quality/ });
		expect(
			icon.compareDocumentPosition(quality) & Node.DOCUMENT_POSITION_FOLLOWING,
		).toBeTruthy();
		await act(async () => engine.stop());
		expect(
			screen.queryByRole("img", { name: "Gapless playback enabled" }),
		).toBeNull();
	});

	it("omits the gapless icon when the engine does not support it", async () => {
		renderPlayerBar(
			undefined,
			new InMemoryPlaybackEngine({ isGapless: false }),
		);
		await act(async () =>
			screen.getByRole("button", { name: "Start track" }).click(),
		);
		expect(
			screen.queryByRole("img", { name: "Gapless playback enabled" }),
		).toBeNull();
	});

	it.each(["Up next", "Next"] as const)(
		"advances to the next Queue item using %s",
		async (control) => {
			const nextTrack = { ...track, id: "track-2", title: "Track 2" };
			const twoItemApi: PlaybackApi = {
				...api,
				getQueue: vi.fn(async () => ({
					items: [
						{ id: "item-1", trackId: track.id, position: 0, track },
						{
							id: "item-2",
							trackId: nextTrack.id,
							position: 1,
							track: nextTrack,
						},
					],
					revision: "1",
				})),
			};
			const engine = new InMemoryPlaybackEngine();
			render(
				<LayoutProvider initialPreferences={defaultPreferences}>
					<PlaybackProvider api={twoItemApi} engine={engine}>
						<PlaybackStarter />
						<PlayerBar />
					</PlaybackProvider>
				</LayoutProvider>,
			);
			await act(async () => {});
			await act(async () => {
				screen.getByRole("button", { name: "Start track" }).click();
			});

			// Whether the Queue is open or closed.
			const toggleQueue = screen.getByRole("button", {
				name: "Toggle queue panel",
			});
			expect(screen.getByTestId("up-next")).toBeTruthy();
			fireEvent.click(toggleQueue);
			const upNext = screen.getByTestId("up-next");
			expect(upNext.textContent).toContain("Track 2");
			await act(async () => {
				fireEvent.click(
					control === "Up next"
						? upNext
						: screen.getByRole("button", { name: "Next" }),
				);
			});
			expect(engine.getState().source).toMatchObject({
				track: { id: "track-2" },
			});
		},
	);

	it("keeps Space controlling playback after pointer queue toggles", async () => {
		const { engine } = renderPlayerBar();
		await act(async () => {});
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		const queueButton = screen.getByRole("button", {
			name: "Toggle queue panel",
		});
		for (const expectedStatus of ["paused", "playing"]) {
			queueButton.focus();
			fireEvent.click(queueButton, { detail: 1 });
			const isExpanded = queueButton.getAttribute("aria-expanded");
			const event = new KeyboardEvent("keydown", {
				key: " ",
				bubbles: true,
				cancelable: true,
			});
			await act(async () => {
				document.activeElement?.dispatchEvent(event);
			});
			expect(event.defaultPrevented).toBe(true);
			expect(engine.getState().status).toBe(expectedStatus);
			expect(queueButton.getAttribute("aria-expanded")).toBe(isExpanded);
		}
	});

	it("preserves queue button focus and native Space for keyboard activation", async () => {
		renderPlayerBar();
		await act(async () => {});
		const queueButton = screen.getByRole("button", {
			name: "Toggle queue panel",
		});
		queueButton.focus();
		const isExpanded = queueButton.getAttribute("aria-expanded");
		fireEvent.click(queueButton, { detail: 0 });
		expect(document.activeElement).toBe(queueButton);
		expect(queueButton.getAttribute("aria-expanded")).not.toBe(isExpanded);
		const event = new KeyboardEvent("keydown", {
			key: " ",
			bubbles: true,
			cancelable: true,
		});
		queueButton.dispatchEvent(event);
		expect(event.defaultPrevented).toBe(false);
	});

	it.each([
		{ length: 3, currentIndex: 1, badge: "1" },
		{ length: 2, currentIndex: 1, badge: "0" },
		{ length: 12, currentIndex: 0, badge: "9+" },
	])(
		"shows upcoming badge $badge for a duplicate queue track",
		async ({ length, currentIndex, badge }) => {
			const items = Array.from({ length }, (_, index) => ({
				id: `item-${index}`,
				trackId: track.id,
				position: index,
				track,
			}));
			const engine = new InMemoryPlaybackEngine();
			render(
				<LayoutProvider
					initialPreferences={{
						...defaultPreferences,
						layout: {
							...defaultPreferences.layout,
							collapsed: { left: false, right: true },
						},
					}}
				>
					<PlaybackProvider
						api={{ ...api, getQueue: async () => ({ items, revision: "1" }) }}
						engine={engine}
					>
						<PlayerBar />
					</PlaybackProvider>
				</LayoutProvider>,
			);
			await act(async () => {
				await engine.play({
					type: "track",
					track,
					queueItemId: items[currentIndex].id,
					playbackUrl: "/stream/track-1",
				});
			});
			const toggle = screen.getByRole("button", { name: "Toggle queue panel" });
			expect(toggle.textContent).toBe(badge);
			expect(toggle.getAttribute("aria-expanded")).toBe("false");
			fireEvent.click(toggle);
			expect(toggle.textContent).toBe("");
			expect(toggle.getAttribute("aria-expanded")).toBe("true");
			expect(toggle.className).toContain("shadow-lit");
		},
	);

	it("offers the Track actions menu inside Now Playing on a phone", async () => {
		vi.stubGlobal("matchMedia", () => ({
			matches: true,
			addEventListener: () => {},
			removeEventListener: () => {},
		}));
		try {
			renderPlayerBar();
			await act(async () => {
				screen.getByRole("button", { name: "Start track" }).click();
			});
			fireEvent.click(screen.getByRole("button", { name: /Open now playing/ }));
			const view = screen.getByRole("dialog", { name: "Now playing" });

			fireEvent.click(
				within(view).getByRole("button", { name: "Track actions" }),
			);
			const menu = within(view).getByRole("menu");
			expect(within(menu).getByText("Sleep timer")).toBeTruthy();

			fireEvent.click(
				within(menu).getByRole("menuitem", { name: "Go to album" }),
			);
			expect(screen.queryByRole("dialog", { name: "Now playing" })).toBeNull();
		} finally {
			vi.unstubAllGlobals();
		}
	});

	it("opens the Now Playing view from the cover and closes it with Escape", async () => {
		renderPlayerBar();
		expect(
			screen.queryByRole("button", { name: "Open now playing" }),
		).toBeNull();
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		fireEvent.click(screen.getByRole("button", { name: "Open now playing" }));
		const view = screen.getByRole("dialog", { name: "Now playing" });
		expect(within(view).getByRole("heading", { name: "Track 1" })).toBeTruthy();
		expect(within(view).getByRole("button", { name: "Pause" })).toBeTruthy();
		expect(within(view).getByTestId("tilting-artwork")).toBeTruthy();
		expect(
			document.activeElement && view.contains(document.activeElement),
		).toBe(true);

		fireEvent.click(within(view).getByRole("button", { name: "Lyrics" }));
		expect(screen.queryByRole("dialog", { name: "Now playing" })).toBeNull();
		const lyricsView = screen.getByRole("dialog", { name: "Lyrics" });
		expect(
			within(lyricsView).getByRole("region", { name: "Current track" }),
		).toBeTruthy();
		expect(
			within(lyricsView).getByRole("region", { name: "Lyrics text" }),
		).toBeTruthy();

		fireEvent.keyDown(document, { key: "Escape" });
		expect(screen.queryByRole("dialog", { name: "Lyrics" })).toBeNull();
	});

	it("gives every Player Bar control an accessible name", async () => {
		renderPlayerBar(
			undefined,
			new InMemoryPlaybackEngine({ outputMode: "system" }),
		);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		const bar = screen.getByTestId("player-bar");
		const unnamed = Array.from(bar.querySelectorAll("button, input")).filter(
			(control) =>
				!(
					control.getAttribute("aria-label") ||
					control.getAttribute("aria-labelledby") ||
					control.textContent?.trim()
				),
		);
		expect(unnamed).toEqual([]);
	});

	it("favorites the current saved Radio Station from the Player Bar", async () => {
		const onToggleStationFavorite = vi.fn();
		render(
			<LayoutProvider initialPreferences={defaultPreferences}>
				<PlaybackProvider api={api} engine={new InMemoryPlaybackEngine()}>
					<RadioStarter />
					<PlayerBar
						isCurrentStationFavorite={false}
						onToggleStationFavorite={onToggleStationFavorite}
					/>
				</PlaybackProvider>
			</LayoutProvider>,
		);
		await act(async () => {
			screen.getByRole("button", { name: "Start radio" }).click();
		});

		fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));
		expect(onToggleStationFavorite).toHaveBeenCalledWith(radioStation.id, true);
	});

	it("announces the new Track to assistive technology", async () => {
		vi.useFakeTimers();
		try {
			renderPlayerBar();
			await act(async () => {
				screen.getByRole("button", { name: "Start track" }).click();
			});
			act(() => {
				vi.advanceTimersByTime(500);
			});
			expect(screen.getByTestId("now-playing-announcer").textContent).toBe(
				"Now playing: Track 1 by Artist",
			);
		} finally {
			vi.useRealTimers();
		}
	});

	it("shows stream details on the quality pill while a Track plays", async () => {
		renderPlayerBar(
			undefined,
			new InMemoryPlaybackEngine({ outputMode: "system" }),
		);
		expect(screen.queryByTestId("quality-details")).toBeNull();

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		const details = screen.getByTestId("quality-details");
		expect(within(details).getByText("Output mode")).toBeTruthy();
		expect(within(details).getByText("Normal")).toBeTruthy();
	});

	it("hides the shortcut button while keeping keyboard help available", () => {
		renderPlayerBar();
		expect(
			screen.queryByRole("button", { name: "Keyboard shortcuts" }),
		).toBeNull();
		fireEvent.keyDown(document.body, { key: "?" });
		expect(
			screen.getByRole("dialog", { name: "Keyboard shortcuts" }),
		).toBeTruthy();

		fireEvent.keyDown(document.body, { key: "Escape" });
		expect(
			screen.queryByRole("dialog", { name: "Keyboard shortcuts" }),
		).toBeNull();

		fireEvent.keyDown(document.body, { key: "?" });
		expect(
			screen.getByRole("dialog", { name: "Keyboard shortcuts" }),
		).toBeTruthy();
	});

	it("seeks and changes volume from the arrow keys", async () => {
		const { engine } = renderPlayerBar();
		const seek = vi.spyOn(engine, "seek");
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		fireEvent.keyDown(document.body, { key: "ArrowRight" });
		expect(seek).toHaveBeenLastCalledWith(5);
		fireEvent.keyDown(document.body, { key: "ArrowRight", shiftKey: true });
		expect(seek).toHaveBeenLastCalledWith(35);
		fireEvent.keyDown(document.body, { key: "ArrowLeft" });
		expect(seek).toHaveBeenLastCalledWith(30);

		fireEvent.keyDown(document.body, { key: "ArrowDown" });
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0.75",
		);
		fireEvent.keyDown(document.body, { key: "m" });
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0",
		);
		fireEvent.keyDown(document.body, { key: "m" });
		expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe(
			"0.75",
		);
	});

	it("renders the actions menu in a portal with anchored coordinates", async () => {
		const rect = {
			top: 320,
			left: 48,
			right: 76,
			bottom: 348,
			width: 28,
			height: 28,
			x: 48,
			y: 320,
			toJSON: () => ({}),
		};
		vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
			rect,
		);

		renderPlayerBar();
		await openActionsMenu();

		const menu = document.getElementById("player-track-actions-menu");
		expect(menu).toBeTruthy();
		expect(menu?.parentElement).toBe(document.body);
		expect(menu?.style.getPropertyValue("--menu-top")).toBe("312px");
		expect(menu?.style.getPropertyValue("--menu-left")).toBe("48px");
	});

	it("navigates to album and artist from the actions menu", async () => {
		renderPlayerBar();
		await openActionsMenu();

		fireEvent.click(screen.getByRole("menuitem", { name: "Go to album" }));
		expect(navigate).toHaveBeenCalledWith({
			to: "/library/$albumId",
			params: { albumId: "album-1" },
		});

		await openActionsMenu();
		fireEvent.click(screen.getByRole("menuitem", { name: "Go to artist" }));
		expect(navigate).toHaveBeenCalledWith({
			to: "/library/tracks",
			search: { artistId: "artist-id" },
		});
	});

	it("offers each Artist credit by name without choosing the combined display credit", async () => {
		const { engine } = renderPlayerBar();
		await act(async () =>
			engine.play({
				type: "track",
				playbackUrl: "/stream/track-1",
				track: {
					...track,
					artistName: "Artist & Guest / Band",
					artists: [
						...track.artists,
						{ id: "guest-id", name: "Guest / Band", role: "main" },
					],
				},
			}),
		);
		fireEvent.click(screen.getByRole("button", { name: "Track actions" }));
		const artists = screen.getByRole("group", { name: "Go to artist" });
		expect(
			within(artists).getByRole("menuitem", { name: "Artist" }),
		).toBeTruthy();
		fireEvent.click(
			within(artists).getByRole("menuitem", { name: "Guest / Band" }),
		);
		expect(navigate).toHaveBeenCalledWith({
			to: "/library/tracks",
			search: { artistId: "guest-id" },
		});
		expect(screen.queryByRole("group", { name: "Go to artist" })).toBeNull();
	});

	it.each([
		{ artists: undefined },
		{ artists: [] },
		{ artists: [{ id: "", name: "Artist" }] },
		{ artists: [{ id: "legacy-artist:Artist", name: "Artist" }] },
	])(
		"disables Go to artist without real credit IDs ($artists)",
		async ({ artists }) => {
			const { engine } = renderPlayerBar();
			await act(async () =>
				engine.play({
					type: "track",
					playbackUrl: "/stream/track-1",
					track: { ...track, artists } as typeof track,
				}),
			);
			fireEvent.click(screen.getByRole("button", { name: "Track actions" }));
			const action = screen.getByRole("menuitem", { name: "Go to artist" });
			expect(action).toHaveProperty("disabled", true);
			fireEvent.click(action);
			expect(navigate).not.toHaveBeenCalled();
		},
	);

	it("opens a hover submenu for add to playlist and toggles membership", async () => {
		const onPlaylistMutated = vi.fn();
		renderPlayerBar(onPlaylistMutated);
		await openPlaylistSubmenu();

		const addToPlaylist = screen.getByRole("menuitem", {
			name: "Add to playlist",
		});
		expect(addToPlaylist.querySelector(".lucide-plus")).toBeNull();
		expect(screen.getByPlaceholderText("Search playlists")).toBeTruthy();
		expect(
			screen.getByRole("menuitem", { name: "Create new playlist" }),
		).toBeTruthy();
		expect(await screen.findByText("Favorites")).toBeTruthy();
		expect(
			await screen.findByRole("menuitem", { name: "Remove from Favorites" }),
		).toBeTruthy();

		await act(async () => {
			fireEvent.click(
				screen.getByRole("menuitem", { name: "Remove from Favorites" }),
			);
		});

		expect(api.removePlaylistTrack).toHaveBeenCalledWith("favorites", track.id);
		expect(onPlaylistMutated).toHaveBeenCalled();

		await act(async () => {
			fireEvent.click(
				screen.getByRole("menuitem", { name: "Add to Favorites" }),
			);
		});

		expect(api.addPlaylistTrack).toHaveBeenCalledWith("favorites", track.id);
	});

	it("keeps the add-to-playlist submenu open while moving across the hover gap", async () => {
		vi.useFakeTimers();
		renderPlayerBar();
		await openPlaylistSubmenu();

		const addToPlaylist = screen.getByRole("menuitem", {
			name: "Add to playlist",
		});
		const wrapper = addToPlaylist.parentElement;
		expect(wrapper).toBeTruthy();
		fireEvent.mouseLeave(wrapper as HTMLElement);
		expect(screen.getByRole("menu", { name: "Add to playlist" })).toBeTruthy();

		fireEvent.mouseEnter(screen.getByRole("menu", { name: "Add to playlist" }));
		await act(async () => {
			vi.advanceTimersByTime(250);
		});

		expect(screen.getByRole("menu", { name: "Add to playlist" })).toBeTruthy();
		vi.useRealTimers();
	});

	it("creates a playlist from the add-to-playlist submenu", async () => {
		renderPlayerBar();
		await openPlaylistSubmenu();
		fireEvent.click(
			screen.getByRole("menuitem", { name: "Create new playlist" }),
		);
		fireEvent.change(screen.getByLabelText("New playlist name"), {
			target: { value: "Late night" },
		});

		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "Create" }));
		});

		expect(api.createPlaylist).toHaveBeenCalledWith("Late night");
		expect(api.addPlaylistTrack).toHaveBeenCalledWith("new", track.id);
	});

	it("filters playlists while searching in the submenu", async () => {
		renderPlayerBar();
		await openPlaylistSubmenu();

		fireEvent.change(screen.getByPlaceholderText("Search playlists"), {
			target: { value: "road" },
		});

		expect(screen.getByText("Road")).toBeTruthy();
		expect(screen.queryByText("Recent")).toBeNull();
		expect(screen.queryByText("Favorites")).toBeNull();
	});

	it("labels a WAV bitrate as read from the container", async () => {
		const format = track.format;
		track.format = "wav";
		try {
			renderPlayerBar();
			await openActionsMenu();
			fireEvent.click(screen.getByRole("menuitem", { name: "Details" }));

			const dialog = screen.getByRole("dialog", { name: "Track 1" });
			expect(within(dialog).getByText("1411 kbps (Native)")).toBeTruthy();
		} finally {
			track.format = format;
		}
	});

	it.each(["Album ensemble", "Artist"])(
		"shows separate Artist and Album Artist details (%s)",
		async (albumArtist) => {
			const { engine } = renderPlayerBar();
			await act(async () =>
				engine.play({
					type: "track",
					playbackUrl: "/stream/track-1",
					track: {
						...track,
						albumArtists: [{ id: "album-artist", name: albumArtist }],
					},
				}),
			);
			fireEvent.click(screen.getByRole("button", { name: "Track actions" }));
			fireEvent.click(screen.getByRole("menuitem", { name: "Details" }));
			const dialog = within(screen.getByRole("dialog", { name: "Track 1" }));
			expect(dialog.getByText("Album Artist").parentElement?.textContent).toBe(
				`Album Artist${albumArtist}`,
			);
			expect(
				dialog
					.getAllByText("Artist")
					.some((node) => node.parentElement?.textContent === "ArtistArtist"),
			).toBe(true);
		},
	);

	it("opens a track info modal from the actions menu", async () => {
		renderPlayerBar();
		await openActionsMenu();
		fireEvent.click(screen.getByRole("menuitem", { name: "Details" }));

		expect(screen.getByRole("dialog", { name: "Track 1" })).toBeTruthy();
		const dialog = screen.getByRole("dialog", { name: "Track 1" });
		expect(within(dialog).getByText("Title")).toBeTruthy();
		expect(within(dialog).getByText("Album 1")).toBeTruthy();
		expect(
			within(dialog).getByText("1411 kbps (Calculated by app)"),
		).toBeTruthy();
		expect(within(dialog).getByText("96 kHz")).toBeTruthy();
		expect(within(dialog).getByText("Track ReplayGain")).toBeTruthy();
		expect(within(dialog).getByText("Gain -7.25 dB")).toBeTruthy();
		// Same row set as the track list "Details" dialog (shared builder).
		expect(within(dialog).getByText("Disc")).toBeTruthy();
		expect(within(dialog).queryByText("Album ReplayGain")).toBeNull();
		expect(within(dialog).queryByText("Unavailable")).toBeNull();
		expect(within(dialog).queryByText("Size")).toBeNull();
		expect(within(dialog).queryByText("Id")).toBeNull();
	});

	it("shows active shuffle and repeat states", async () => {
		renderPlayerBar();

		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		fireEvent.click(screen.getByRole("button", { name: "Shuffle off" }));
		expect(screen.getByRole("button", { name: "Shuffle on" })).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Repeat off" }));
		expect(screen.getByRole("button", { name: "Repeat once" })).toBeTruthy();
		// Repeat once shows the repeat-1 glyph rather than a number badge.
		expect(
			screen
				.getByRole("button", { name: "Repeat once" })
				.querySelector(".lucide-repeat-1"),
		).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Repeat once" }));
		expect(screen.getByRole("button", { name: "Repeat loop" })).toBeTruthy();
		expect(screen.getByLabelText("Repeat infinitely")).toBeTruthy();
	});

	it("routes Previous, Next, and Shuffle through public engine controls", async () => {
		const engine = new InMemoryPlaybackEngine();
		const previous = vi.spyOn(engine, "previous");
		const next = vi.spyOn(engine, "next");
		const toggleShuffle = vi.spyOn(engine, "toggleShuffle");
		renderPlayerBar(undefined, engine);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});

		fireEvent.click(screen.getByRole("button", { name: "Previous" }));
		fireEvent.click(screen.getByRole("button", { name: "Next" }));
		fireEvent.click(screen.getByRole("button", { name: "Shuffle off" }));

		expect(previous).toHaveBeenCalledOnce();
		expect(next).toHaveBeenCalledOnce();
		expect(toggleShuffle).toHaveBeenCalledOnce();
	});

	it("restarts the Track on Previous after 30 seconds", async () => {
		const engine = new InMemoryPlaybackEngine();
		const previous = vi.spyOn(engine, "previous");
		const seek = vi.spyOn(engine, "seek");
		renderPlayerBar(undefined, engine);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		await act(async () => engine.seek(31));
		seek.mockClear();

		fireEvent.click(screen.getByRole("button", { name: "Previous" }));

		expect(seek).toHaveBeenCalledWith(0);
		expect(previous).not.toHaveBeenCalled();
	});

	it("restarts the Track on the Previous shortcut after 30 seconds", async () => {
		const engine = new InMemoryPlaybackEngine();
		const previous = vi.spyOn(engine, "previous");
		const seek = vi.spyOn(engine, "seek");
		renderPlayerBar(undefined, engine);
		await act(async () => {
			screen.getByRole("button", { name: "Start track" }).click();
		});
		await act(async () => engine.seek(31));
		seek.mockClear();

		fireEvent.keyDown(document.body, { key: "MediaTrackPrevious" });

		expect(seek).toHaveBeenCalledWith(0);
		expect(previous).not.toHaveBeenCalled();
	});

	it.each([
		["mpv-crash-loop", "Native playback stopped after repeated failures."],
		["mpv-restart-failed", "Native playback could not restart. Try again."],
	] as const)("renders %s as an actionable alert", async (code, message) => {
		const engine = new InMemoryPlaybackEngine();
		renderPlayerBar(undefined, engine);

		await act(async () => engine.fail({ code, message }));

		expect(screen.getByRole("alert").textContent).toContain(message);
	});
});

describe("PlayerBar during a Room", () => {
	const room: ListenTogetherRoom = {
		revision: 1,
		trackIds: [track.id],
		currentIndex: 0,
		playing: true,
		positionMs: 0,
		positionAtMs: 0,
		lyricsOpen: false,
		guestCount: 1,
		maxGuests: 8,
		expiresAt: "2026-10-31T12:00:00Z",
	};
	const sendCommand = vi.fn(async () => undefined);

	function roomPlayback(overrides: Partial<RoomPlayback> = {}): RoomPlayback {
		return { role: "owner", room, queue: [], sendCommand, ...overrides };
	}

	function tree(engine: InMemoryPlaybackEngine, value: RoomPlayback) {
		return (
			<LayoutProvider initialPreferences={defaultPreferences}>
				<RoomPlaybackContext.Provider value={value}>
					<PlaybackProvider api={api} engine={engine}>
						<PlayerBar />
					</PlaybackProvider>
				</RoomPlaybackContext.Provider>
			</LayoutProvider>
		);
	}

	async function renderInRoom(value = roomPlayback()) {
		const engine = new InMemoryPlaybackEngine();
		await engine.play({
			type: "track",
			track,
			playbackUrl: `/stream/${track.id}`,
		});
		const view = render(tree(engine, value));
		await act(async () => undefined);
		return { ...view, engine };
	}

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("takes whether the lyrics are open from the Room", async () => {
		const { rerender, engine } = await renderInRoom(
			roomPlayback({ room: { ...room, lyricsOpen: true } }),
		);
		expect(screen.getByRole("dialog", { name: "Lyrics" })).toBeTruthy();

		rerender(tree(engine, roomPlayback()));

		expect(screen.queryByRole("dialog", { name: "Lyrics" })).toBeNull();
	});

	it("sends opening the lyrics to the Room instead of opening them here", async () => {
		await renderInRoom();

		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));

		expect(sendCommand).toHaveBeenCalledWith({
			type: "setLyricsOpen",
			open: true,
		});
		expect(screen.queryByRole("dialog", { name: "Lyrics" })).toBeNull();
	});

	it("sends closing the lyrics to the Room", async () => {
		await renderInRoom(roomPlayback({ room: { ...room, lyricsOpen: true } }));

		fireEvent.keyDown(document, { key: "Escape" });
		await waitFor(() =>
			expect(sendCommand).toHaveBeenCalledWith({
				type: "setLyricsOpen",
				open: false,
			}),
		);
	});

	it("toggles the lyrics from the keyboard through the Room", async () => {
		await renderInRoom();

		fireEvent.keyDown(document, { key: "l" });

		expect(sendCommand).toHaveBeenCalledWith({
			type: "setLyricsOpen",
			open: true,
		});
	});

	it("makes a click on a synced line a Room seek", async () => {
		vi.mocked(api.getTrackLyrics)?.mockResolvedValueOnce({
			lyrics: "[00:10.00] First line\n[00:20.00] Second line",
		});
		const { engine } = await renderInRoom(
			roomPlayback({ room: { ...room, lyricsOpen: true } }),
		);
		const seek = vi.spyOn(engine, "seek");

		fireEvent.click(await screen.findByRole("button", { name: "Second line" }));

		expect(sendCommand).toHaveBeenCalledWith({
			type: "seek",
			positionMs: 20_000,
		});
		expect(seek).not.toHaveBeenCalled();
	});

	it("keeps the lyrics local outside a Room", async () => {
		await renderInRoom(roomPlayback({ room: null }));

		fireEvent.click(screen.getByRole("button", { name: "Lyrics" }));

		expect(screen.getByRole("dialog", { name: "Lyrics" })).toBeTruthy();
		expect(sendCommand).not.toHaveBeenCalled();
	});

	it("offers no Queue, speed or timer actions in a Room", async () => {
		await renderInRoom();

		fireEvent.click(screen.getByRole("button", { name: "Track actions" }));

		expect(screen.queryByRole("menuitem", { name: "Play next" })).toBeNull();
		expect(screen.queryByRole("menuitem", { name: "Add to queue" })).toBeNull();
		expect(screen.queryByRole("group", { name: "Speed" })).toBeNull();
		expect(screen.queryByRole("group", { name: "Sleep timer" })).toBeNull();
		expect(screen.queryByRole("group", { name: "A-B repeat" })).toBeNull();
		expect(screen.getByRole("menuitem", { name: "Go to album" })).toBeTruthy();
	});
});
