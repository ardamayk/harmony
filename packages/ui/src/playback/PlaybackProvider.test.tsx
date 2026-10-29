import {
	ApiError,
	type ExternalTrack,
	type ExternalTrackPlayback,
	type QueueEvent,
	type RadioSearchResult,
	type RadioStation,
} from "@repo/api-client";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	type PlaybackApi,
	PlaybackProvider,
	usePlayback,
} from "./PlaybackProvider";
import { InMemoryPlaybackEngine } from "./testing/InMemoryPlaybackEngine";

const toastError = vi.fn();
vi.mock("sonner", () => ({
	toast: {
		error: (...args: unknown[]) => toastError(...args),
		success: vi.fn(),
	},
}));

const track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "flac",
	genres: [],
};

const track2 = {
	...track,
	id: "track-2",
	title: "Track 2",
	durationMs: 180000,
};

const track3 = {
	...track,
	id: "track-3",
	title: "Track 3",
	durationMs: 240000,
};

const radioStation: RadioStation = {
	id: "station-1",
	name: "Station 1",
	streamUrl: "https://example.com/live",
	tags: [],
	source: "manual",
	isFavorite: false,
	position: 0,
};

const catalogResult: RadioSearchResult = {
	stationUuid: "catalog-1",
	name: "Catalog 1",
	streamUrl: "https://example.com/preview",
	tags: [],
};

const externalTrack: ExternalTrack = {
	id: "external-1",
	title: "External Song",
	artistName: "Uploader",
	durationMs: 90000,
	webpageUrl: "https://soundcloud.com/uploader/external-song",
	extractor: "Soundcloud",
	hasArtwork: true,
};

const queueItems = [
	{ id: "item-1", trackId: track.id, position: 0, track },
	{ id: "item-2", trackId: track2.id, position: 1, track: track2 },
];

function createApi(items = queueItems): PlaybackApi {
	return {
		getQueue: vi.fn(async () => ({ items, revision: "1" })),
		replaceQueue: vi.fn(async (trackIds: string[]) => ({
			items: trackIds.map((trackId, position) => ({
				id: `item-${position + 1}`,
				trackId,
				position,
				track: trackId === track2.id ? track2 : track,
			})),
			revision: "2",
		})),
		reorderQueue: vi.fn(async () => ({ items, revision: "2" })),
		appendQueueItem: vi.fn(async () => ({ items: queueItems, revision: "2" })),
		removeQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
		clearQueue: vi.fn(async (afterItemId: string | null) => ({
			items: items.slice(
				0,
				items.findIndex((item) => item.id === afterItemId) + 1,
			),
			revision: "2",
		})),
		subscribeQueueEvents: vi.fn(() => vi.fn()),
		getStreamUrl: (trackId) => `/stream/${trackId}`,
		getAlbumCoverUrl: (albumId) => `/cover/${albumId}`,
		getRadioStationStreamUrl: (stationId) => `/radio/${stationId}`,
		getRadioCatalogPreviewStreamUrl: (stationUuid) =>
			`/radio/preview/${stationUuid}`,
		getRadioNowPlaying: vi.fn(async () => ({ raw: "Artist - Song" })),
		getExternalTrackPlayback: vi.fn(async (externalTrackId: string) => ({
			format: "hls" as const,
			streamPath: `/api/v1/external/tracks/${externalTrackId}/playlist.m3u8`,
		})),
		getExternalStreamUrl: (streamPath: string) => `/server${streamPath}`,
		getExternalTrackArtworkUrl: (externalTrackId: string) =>
			`/server/api/v1/external/tracks/${externalTrackId}/artwork`,
		listPlaylists: vi.fn(async () => ({ items: [], total: 0 })),
		getPlaylist: vi.fn(async (playlistId: string) => ({
			id: playlistId,
			name: "Playlist",
			isDefault: false,
			trackCount: 0,
			tracks: [],
		})),
		createPlaylist: vi.fn(async (name: string) => ({
			id: "playlist-1",
			name,
			isDefault: false,
			trackCount: 0,
		})),
		addPlaylistTrack: vi.fn(async () => ({
			id: "playlist-1",
			name: "Playlist",
			isDefault: false,
			trackCount: 1,
			tracks: [track],
		})),
		removePlaylistTrack: vi.fn(async () => ({
			id: "playlist-1",
			name: "Playlist",
			isDefault: false,
			trackCount: 0,
			tracks: [],
		})),
	};
}

function Harness() {
	const playback = usePlayback();
	return (
		<div>
			<span data-testid="queue">
				{playback.queue.map((item) => item.track.id).join(",")}
			</span>
			<span data-testid="track">{playback.currentTrack?.title ?? ""}</span>
			<span data-testid="radio">
				{playback.currentRadioStation?.name ?? ""}
			</span>
			<span data-testid="external">
				{playback.currentExternalTrack?.title ?? ""}
			</span>
			<span data-testid="now-playing">
				{playback.radioNowPlaying?.raw ?? ""}
			</span>
			<span data-testid="playing">{String(playback.isPlaying)}</span>
			<span data-testid="reconnecting">{String(playback.isReconnecting)}</span>
			<span data-testid="volume">{playback.volume}</span>
			<span data-testid="repeat">{playback.repeatMode}</span>
			<span data-testid="queue-conflict">{playback.queueConflict ?? ""}</span>
			<span data-testid="error-cause">
				{playback.errorRecovery?.cause ?? ""}
			</span>
			<span data-testid="error-countdown">
				{playback.errorRecovery?.countdownSeconds ?? ""}
			</span>
			<button type="button" onClick={() => playback.errorRecovery?.retry()}>
				Retry
			</button>
			<button type="button" onClick={() => playback.errorRecovery?.skip()}>
				Skip
			</button>
			<button
				type="button"
				onClick={() => playback.errorRecovery?.cancelCountdown()}
			>
				Wait
			</button>
			<button type="button" onClick={() => void playback.playNext(track.id)}>
				Play next
			</button>
			<button type="button" onClick={() => void playback.playTrack(track.id)}>
				Track
			</button>
			<button type="button" onClick={() => void playback.playTrack(track2.id)}>
				Track 2
			</button>
			<button
				type="button"
				onClick={() => void playback.playTrack(track2.id, [track2.id])}
			>
				Play from album
			</button>
			<button
				type="button"
				onClick={() =>
					void playback.playTrack(track2.id, [track2.id], {
						kind: "playlist",
						playlistId: "p1",
						name: "Favorites",
					})
				}
			>
				Play playlist
			</button>
			<button
				type="button"
				onClick={() =>
					void playback.queueTracks([track.id, track2.id], {
						kind: "playlist",
						playlistId: "p1",
						name: "Favorites",
					})
				}
			>
				Queue playlist
			</button>
			<button type="button" onClick={() => void playback.addToQueue(track.id)}>
				Add to queue
			</button>
			<button type="button" onClick={() => void playback.playQueueIndex(0)}>
				First Queue item
			</button>
			<button
				type="button"
				onClick={() => void playback.playRadioStation(radioStation)}
			>
				Radio
			</button>
			<button
				type="button"
				onClick={() => void playback.playRadioCatalogPreview(catalogResult)}
			>
				Preview
			</button>
			<button
				type="button"
				onClick={() =>
					void playback.playExternalTrack(externalTrack).catch(() => undefined)
				}
			>
				External
			</button>
			<button type="button" onClick={() => playback.setVolume(0.3)}>
				Volume
			</button>
			<button type="button" onClick={playback.cycleRepeatMode}>
				Repeat
			</button>
			<button type="button" onClick={() => void playback.clearQueue()}>
				Clear
			</button>
			<button
				type="button"
				onClick={() => void playback.queueTracks([track2.id])}
			>
				Append
			</button>
			<button
				type="button"
				onClick={() => void playback.removeFromQueue("item-1")}
			>
				Remove
			</button>
			<button
				type="button"
				onClick={() => void playback.reorderQueue(["item-2", "item-1"])}
			>
				Reorder
			</button>
		</div>
	);
}

function renderPlayback(
	api = createApi(),
	engine: InMemoryPlaybackEngine = new InMemoryPlaybackEngine(),
) {
	render(
		<PlaybackProvider api={api} engine={engine}>
			<Harness />
		</PlaybackProvider>,
	);
	return { api, engine };
}

afterEach(cleanup);

describe("PlaybackProvider", () => {
	it("explains a missing file and skips after the countdown", async () => {
		vi.useFakeTimers();
		try {
			const api = createApi();
			api.headTrackStream = vi.fn(async () => ({ status: 404 }));
			const { engine } = renderPlayback(api);
			await act(async () => {});
			await act(async () =>
				screen.getByRole("button", { name: "Track" }).click(),
			);

			await act(async () =>
				engine.fail({ code: "playback-failed", message: "Playback failed" }),
			);
			await act(async () => {});

			expect(api.headTrackStream).toHaveBeenCalledWith("track-1");
			expect(screen.getByTestId("error-cause").textContent).toBe(
				"file-missing",
			);
			expect(screen.getByTestId("error-countdown").textContent).toBe("5");
			expect(engine.getState().source).toMatchObject({
				track: { id: "track-1" },
			});

			// Each tick schedules the next one from an effect, so advance the
			// clock one second per act.
			for (let tick = 0; tick < 5; tick += 1) {
				await act(async () => {
					vi.advanceTimersByTime(1000);
				});
			}
			await act(async () => {});
			expect(engine.getState().source).toMatchObject({
				track: { id: "track-2" },
			});
			expect(screen.getByTestId("error-cause").textContent).toBe("");
		} finally {
			vi.useRealTimers();
		}
	});

	it("lets the user retry a failed Track and stop the countdown", async () => {
		vi.useFakeTimers();
		try {
			const api = createApi();
			api.headTrackStream = vi.fn(async () => {
				throw new Error("offline");
			});
			const { engine } = renderPlayback(api);
			await act(async () => {});
			await act(async () =>
				screen.getByRole("button", { name: "Track" }).click(),
			);
			await act(async () =>
				engine.fail({ code: "playback-failed", message: "Playback failed" }),
			);
			await act(async () => {});
			expect(screen.getByTestId("error-cause").textContent).toBe("network");

			await act(async () =>
				screen.getByRole("button", { name: "Wait" }).click(),
			);
			await act(async () => {
				vi.advanceTimersByTime(10000);
			});
			expect(engine.getState().source).toMatchObject({
				track: { id: "track-1" },
			});
			expect(engine.getState().status).toBe("error");

			const play = vi.spyOn(engine, "play");
			await act(async () =>
				screen.getByRole("button", { name: "Retry" }).click(),
			);
			expect(play).toHaveBeenCalledWith(
				expect.objectContaining({
					track: expect.objectContaining({ id: "track-1" }),
				}),
			);
			expect(engine.getState().status).toBe("playing");
			expect(screen.getByTestId("error-cause").textContent).toBe("");
		} finally {
			vi.useRealTimers();
		}
	});

	it("skips immediately from the recovery controls and asks a native engine to advance", async () => {
		const api = createApi();
		const engine = new InMemoryPlaybackEngine();
		const next = vi.spyOn(engine, "next");
		Object.assign(engine, { syncQueueContext: vi.fn(async () => undefined) });
		renderPlayback(api, engine);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () =>
			engine.fail({ code: "playback-failed", message: "Playback failed" }),
		);
		await act(async () => screen.getByRole("button", { name: "Skip" }).click());
		expect(next).toHaveBeenCalledOnce();
	});

	it("plays previous and next Queue items from native tray navigation", async () => {
		const { engine } = renderPlayback();
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		await act(async () => engine.navigate("next"));
		expect(engine.getState().source).toMatchObject({
			type: "track",
			track: { id: "track-2" },
		});

		await act(async () => engine.navigate("previous"));
		expect(engine.getState().source).toMatchObject({
			type: "track",
			track: { id: "track-1" },
		});
	});

	it("maps queued Tracks to PlaybackEngine sources", async () => {
		const { engine } = renderPlayback();
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		expect(engine.getState().source).toEqual({
			type: "track",
			track,
			playbackUrl: "/stream/track-1",
			queueItemId: "item-1",
			artworkUrl: "/cover/album-1",
		});
		expect(screen.getByTestId("track").textContent).toBe("Track 1");
		expect(screen.getByTestId("playing").textContent).toBe("true");
	});

	it("plays Radio Stations and Catalog Previews without changing Queue", async () => {
		const api = createApi();
		const { engine } = renderPlayback(api);
		await act(async () => {});
		const queueBeforePlayback = screen.getByTestId("queue").textContent;
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () =>
			screen.getByRole("button", { name: "Radio" }).click(),
		);
		expect(engine.getState().source).toMatchObject({
			type: "radio-station",
			playbackUrl: "/radio/station-1",
			sourceUrl: radioStation.streamUrl,
		});
		expect(screen.getByTestId("track").textContent).toBe("");
		expect(screen.getByTestId("radio").textContent).toBe("Station 1");
		await waitFor(() => {
			expect(screen.getByTestId("now-playing").textContent).toBe(
				"Artist - Song",
			);
		});
		expect(api.getRadioNowPlaying).toHaveBeenCalledWith(radioStation.id);

		await act(async () =>
			screen.getByRole("button", { name: "Preview" }).click(),
		);
		expect(engine.getState().source).toMatchObject({
			type: "catalog-preview",
			playbackUrl: "/radio/preview/catalog-1",
			sourceUrl: catalogResult.streamUrl,
		});
		expect(screen.getByTestId("track").textContent).toBe("");
		expect(screen.getByTestId("radio").textContent).toBe("Catalog 1");
		expect(screen.getByTestId("now-playing").textContent).toBe("");
		expect(api.getRadioNowPlaying).toHaveBeenCalledTimes(1);
		expect(screen.getByTestId("queue").textContent).toBe(queueBeforePlayback);
		expect(api.replaceQueue).not.toHaveBeenCalled();
		expect(api.appendQueueItem).not.toHaveBeenCalled();
		expect(api.removeQueueItem).not.toHaveBeenCalled();
		expect(api.reorderQueue).not.toHaveBeenCalled();

		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		expect(engine.getState().source).toMatchObject({
			type: "track",
			track: { id: track.id },
		});
		expect(screen.getByTestId("radio").textContent).toBe("");
		expect(screen.getByTestId("queue").textContent).toBe(queueBeforePlayback);
	});

	it("plays an External Track outside the Queue and stops at its end", async () => {
		const api = createApi();
		const { engine } = renderPlayback(api);
		await act(async () => {});
		const queueBeforePlayback = screen.getByTestId("queue").textContent;
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		await act(async () =>
			screen.getByRole("button", { name: "External" }).click(),
		);

		expect(api.getExternalTrackPlayback).toHaveBeenCalledWith("external-1");
		const streamUrl = "/server/api/v1/external/tracks/external-1/playlist.m3u8";
		expect(engine.getState().source).toEqual({
			type: "external-track",
			externalTrack,
			playbackUrl: streamUrl,
			sourceUrl: streamUrl,
			// OS media controls show the Music Server's copy of its cover.
			artworkUrl: "/server/api/v1/external/tracks/external-1/artwork",
		});
		expect(engine.getState().duration).toBe(90);
		expect(screen.getByTestId("external").textContent).toBe("External Song");
		expect(screen.getByTestId("track").textContent).toBe("");
		expect(screen.getByTestId("radio").textContent).toBe("");
		expect(api.getRadioNowPlaying).not.toHaveBeenCalled();

		await act(async () => engine.finish());

		expect(engine.getState()).toMatchObject({
			status: "ended",
			source: { type: "external-track" },
		});
		expect(screen.getByTestId("queue").textContent).toBe(queueBeforePlayback);
		expect(api.appendQueueItem).not.toHaveBeenCalled();
		expect(api.replaceQueue).not.toHaveBeenCalled();
	});

	it("rejects when an External Track cannot be resolved", async () => {
		const api = createApi();
		const failure = new Error("unplayable");
		api.getExternalTrackPlayback = vi.fn(async () => {
			throw failure;
		});
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		const onRejected = vi.fn();
		function Trigger() {
			const { playExternalTrack } = usePlayback();
			return (
				<button
					type="button"
					onClick={() =>
						void playExternalTrack(externalTrack).catch(onRejected)
					}
				>
					Play External
				</button>
			);
		}
		const engine = new InMemoryPlaybackEngine();
		render(
			<PlaybackProvider api={api} engine={engine}>
				<Trigger />
			</PlaybackProvider>,
		);

		await act(async () =>
			screen.getByRole("button", { name: "Play External" }).click(),
		);

		expect(onRejected).toHaveBeenCalledWith(failure);
		expect(engine.getState().source).toBeNull();
		warn.mockRestore();
	});

	describe("a choice made while an External Track resolves", () => {
		const otherExternalTrack: ExternalTrack = {
			...externalTrack,
			id: "external-2",
			title: "Other External Song",
		};

		/** Resolves each External Track only when the test says so. */
		function renderWithSlowResolves() {
			const api = createApi();
			const resolvers = new Map<
				string,
				{ resolve: () => void; reject: (error: Error) => void }
			>();
			api.getExternalTrackPlayback = vi.fn(
				(externalTrackId: string) =>
					new Promise<ExternalTrackPlayback>((resolve, reject) => {
						resolvers.set(externalTrackId, {
							resolve: () =>
								resolve({
									format: "hls" as const,
									streamPath: `/api/v1/external/tracks/${externalTrackId}/playlist.m3u8`,
								}),
							reject,
						});
					}),
			);
			const onRejected = vi.fn();
			function Trigger() {
				const playback = usePlayback();
				return (
					<div>
						<button
							type="button"
							onClick={() =>
								void playback.playExternalTrack(externalTrack).catch(onRejected)
							}
						>
							External 1
						</button>
						<button
							type="button"
							onClick={() =>
								void playback
									.playExternalTrack(otherExternalTrack)
									.catch(onRejected)
							}
						>
							External 2
						</button>
						<button
							type="button"
							onClick={() => void playback.playTrack(track.id)}
						>
							Track
						</button>
						<button type="button" onClick={playback.stopPlayback}>
							Stop
						</button>
					</div>
				);
			}
			const engine = new InMemoryPlaybackEngine();
			render(
				<PlaybackProvider api={api} engine={engine}>
					<Trigger />
				</PlaybackProvider>,
			);
			const click = (name: string) =>
				act(async () => screen.getByRole("button", { name }).click());
			const settle = (externalTrackId: string, error?: Error) =>
				act(async () => {
					const resolver = resolvers.get(externalTrackId);
					if (error) resolver?.reject(error);
					else resolver?.resolve();
				});
			return { engine, onRejected, click, settle };
		}

		it("keeps a Track chosen after the External Track", async () => {
			const { engine, onRejected, click, settle } = renderWithSlowResolves();
			await act(async () => {});
			const play = vi.spyOn(engine, "play");
			await click("External 1");
			await click("Track");
			await settle("external-1");

			expect(engine.getState().source).toMatchObject({
				type: "track",
				track: { id: track.id },
			});
			expect(play).toHaveBeenCalledTimes(1);
			expect(onRejected).not.toHaveBeenCalled();
		});

		it("plays the newer External Track whichever resolves first", async () => {
			for (const order of [
				["external-2", "external-1"],
				["external-1", "external-2"],
			]) {
				const { engine, click, settle } = renderWithSlowResolves();
				await click("External 1");
				await click("External 2");
				for (const externalTrackId of order) await settle(externalTrackId);

				expect(engine.getState().source).toMatchObject({
					type: "external-track",
					externalTrack: { id: "external-2" },
				});
				cleanup();
			}
		});

		it("stays stopped after Stop, and drops a late failure", async () => {
			const { engine, onRejected, click, settle } = renderWithSlowResolves();
			const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
			await click("External 1");
			await click("Stop");
			await settle("external-1");
			expect(engine.getState().source).toBeNull();

			await click("External 2");
			await click("Stop");
			await settle("external-2", new Error("unplayable"));
			expect(engine.getState().source).toBeNull();
			expect(onRejected).not.toHaveBeenCalled();
			expect(warn).not.toHaveBeenCalled();
			warn.mockRestore();
		});
	});

	it("exposes an active reconnecting Radio Station session", async () => {
		const { engine } = renderPlayback();
		await act(async () =>
			screen.getByRole("button", { name: "Radio" }).click(),
		);

		act(() => engine.reconnect());

		expect(screen.getByTestId("playing").textContent).toBe("true");
		expect(screen.getByTestId("reconnecting").textContent).toBe("true");
	});

	it("delegates Playback Session controls to PlaybackEngine", async () => {
		const { engine } = renderPlayback();
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () => {
			screen.getByRole("button", { name: "Volume" }).click();
			screen.getByRole("button", { name: "Repeat" }).click();
		});

		expect(engine.getState()).toMatchObject({
			volume: 0.3,
			repeatMode: "once",
		});
		expect(screen.getByTestId("volume").textContent).toBe("0.3");
		expect(screen.getByTestId("repeat").textContent).toBe("once");
	});

	it("inserts a duplicate after the playing entry without replacing item identities", async () => {
		const api = createApi();
		const appended = [
			...queueItems,
			{ ...queueItems[0], id: "new-item", position: 2 },
		];
		api.appendQueueItem = vi.fn(async () => ({
			items: appended,
			revision: "2",
		}));
		api.reorderQueue = vi.fn(async (ids: string[]) => ({
			items: ids.map((id, position) => {
				const item = appended.find((item) => item.id === id);
				if (!item) throw new Error(`Unknown queue item: ${id}`);
				return { ...item, position };
			}),
			revision: "3",
		}));
		const { engine } = renderPlayback(api);
		await act(async () =>
			screen.getByRole("button", { name: /^Track$/ }).click(),
		);
		await act(async () =>
			screen.getByRole("button", { name: /^Play next$/ }).click(),
		);
		expect(api.replaceQueue).not.toHaveBeenCalled();
		expect(api.reorderQueue).toHaveBeenCalledWith(
			["item-1", "new-item", "item-2"],
			"2",
		);
		expect(engine.getState().source).toMatchObject({ queueItemId: "item-1" });
		await act(async () => engine.finish());
		expect(engine.getState().source).toMatchObject({ queueItemId: "new-item" });
	});

	it("does not coordinate queue, repeat, or error skipping from an observer window", async () => {
		const engine = new InMemoryPlaybackEngine();
		await engine.play({
			type: "track",
			track,
			playbackUrl: "/stream/track-1",
			queueItemId: "item-1",
		});
		engine.cycleRepeatMode();
		const cycleRepeatMode = vi.spyOn(engine, "cycleRepeatMode");
		const next = vi.spyOn(engine, "next");
		const syncQueueContext = vi.fn(async () => {});
		Object.assign(engine, { syncQueueContext });
		render(
			<PlaybackProvider
				api={createApi()}
				engine={engine}
				shouldCoordinateQueue={false}
			>
				<Harness />
			</PlaybackProvider>,
		);
		await waitFor(() =>
			expect(screen.getByTestId("queue").textContent).toBe("track-1,track-2"),
		);
		expect(syncQueueContext).not.toHaveBeenCalled();
		expect(cycleRepeatMode).not.toHaveBeenCalled();
		await act(async () =>
			engine.fail({ code: "playback-failed", message: "Playback failed" }),
		);
		expect(screen.getByTestId("error-countdown").textContent).toBe("");
		expect(next).not.toHaveBeenCalled();
	});

	it("advances Queue after engine reports Track ended", async () => {
		const { engine } = renderPlayback();
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () => engine.finish());

		expect(engine.getState().source).toMatchObject({
			type: "track",
			track: { id: "track-2" },
		});
	});

	it("delegates Queue context and end advancement to a native engine", async () => {
		const engine = new InMemoryPlaybackEngine();
		const syncQueueContext = vi.fn(async () => {});
		Object.assign(engine, { syncQueueContext });
		renderPlayback(createApi(), engine);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		expect(syncQueueContext).toHaveBeenLastCalledWith(
			[
				expect.objectContaining({ queueItemId: "item-1" }),
				expect.objectContaining({ queueItemId: "item-2" }),
			],
			0,
		);
		await act(async () => engine.finish());
		expect(engine.getState().status).toBe("ended");
		expect(engine.getState().source).toMatchObject({
			track: { id: "track-1" },
		});
	});

	it("advances duplicate Tracks by Queue item identity", async () => {
		const duplicateItems = [
			{ id: "item-1", trackId: track.id, position: 0, track },
			{ id: "item-2", trackId: track.id, position: 1, track },
			{ id: "item-3", trackId: track2.id, position: 2, track: track2 },
		];
		const { engine } = renderPlayback(createApi(duplicateItems));
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "First Queue item" }).click(),
		);

		await act(async () => engine.finish());
		await act(async () => engine.finish());

		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-2" } },
			status: "playing",
		});
	});

	it("removes upcoming Tracks on Clear and lets the playing one finish", async () => {
		const api = createApi();
		const { engine } = renderPlayback(api);
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () =>
			screen.getByRole("button", { name: "Clear" }).click(),
		);

		expect(api.clearQueue).toHaveBeenCalledWith("item-1", "2");
		expect(screen.getByTestId("queue").textContent).toBe("track-1");
		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-1" } },
			status: "playing",
		});

		await act(async () => engine.finish());
		expect(engine.getState()).toMatchObject({ source: null, status: "idle" });
	});

	it("keeps played Tracks before the playing one after Clear", async () => {
		const api = createApi();
		renderPlayback(api);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track 2" }).click(),
		);

		await act(async () =>
			screen.getByRole("button", { name: "Clear" }).click(),
		);

		expect(api.clearQueue).toHaveBeenCalledWith("item-2", "1");
		expect(screen.getByTestId("queue").textContent).toBe("track-1,track-2");
	});

	it("clears every Queue item when nothing is playing", async () => {
		const api = createApi();
		renderPlayback(api);
		await act(async () => {});

		await act(async () =>
			screen.getByRole("button", { name: "Clear" }).click(),
		);

		expect(api.clearQueue).toHaveBeenCalledWith(null, "1");
		expect(screen.getByTestId("queue").textContent).toBe("");
	});

	it("keeps current audio after remote removal and resolves latest Queue when it ends", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: [queueItems[1]], revision: "2" });
		const { engine } = renderPlayback(api);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		await notifyQueueEvent(api, {
			revision: "2",
			sequence: "2",
			invalidates: ["queue"],
		});
		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-1" } },
			status: "playing",
		});

		await act(async () => engine.finish());
		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-2" } },
			status: "playing",
		});
	});

	it("advances from a remotely removed non-head Track to its former successor", async () => {
		const threeItemQueue = [
			queueItems[0],
			queueItems[1],
			{ id: "item-3", trackId: track3.id, position: 2, track: track3 },
		];
		const api = createApi(threeItemQueue);
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: threeItemQueue, revision: "1" })
			.mockResolvedValueOnce({
				items: [threeItemQueue[0], threeItemQueue[2]],
				revision: "2",
			});
		const { engine } = renderPlayback(api);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track 2" }).click(),
		);

		await notifyQueueEvent(api, {
			revision: "2",
			sequence: "2",
			invalidates: ["queue"],
		});
		await act(async () => engine.finish());

		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-3" } },
			status: "playing",
		});
	});

	it("keeps current audio after remote Queue clear and stops when it ends", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: [], revision: "2" });
		const { engine } = renderPlayback(api);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);

		await notifyQueueEvent(api, {
			revision: "2",
			sequence: "2",
			invalidates: ["queue"],
		});
		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-1" } },
			status: "playing",
		});

		await act(async () => engine.finish());
		expect(engine.getState()).toMatchObject({ source: null, status: "idle" });
	});

	it.each([
		["once", 1],
		["loop", 2],
	] as const)(
		"remote Queue clear overrides %s repeat after current Track",
		async (_repeatMode, repeatClicks) => {
			const api = createApi();
			vi.mocked(api.getQueue)
				.mockResolvedValueOnce({ items: queueItems, revision: "1" })
				.mockResolvedValueOnce({ items: [], revision: "2" });
			const { engine } = renderPlayback(api);
			await act(async () => {});
			await act(async () =>
				screen.getByRole("button", { name: "Track" }).click(),
			);
			await act(async () => {
				for (let clickIndex = 0; clickIndex < repeatClicks; clickIndex += 1) {
					screen.getByRole("button", { name: "Repeat" }).click();
				}
			});

			await notifyQueueEvent(api, {
				revision: "2",
				sequence: "2",
				invalidates: ["queue"],
			});
			expect(engine.getState()).toMatchObject({
				source: { type: "track", track: { id: "track-1" } },
				status: "playing",
				repeatMode: "off",
			});
			await act(async () => engine.finish());

			expect(engine.getState()).toMatchObject({ source: null, status: "idle" });
		},
	);

	it("never applies Queue events to device-local Playback Session state", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: [queueItems[1]], revision: "2" });
		const { engine } = renderPlayback(api);
		await act(async () => {});
		await act(async () =>
			screen.getByRole("button", { name: "Track" }).click(),
		);
		await act(async () => {
			engine.seek(42);
			engine.setVolume(0.3);
			engine.togglePlay();
		});

		await notifyQueueEvent(api, {
			revision: "2",
			sequence: "2",
			invalidates: ["queue"],
		});

		expect(engine.getState()).toMatchObject({
			source: { type: "track", track: { id: "track-1" } },
			status: "paused",
			currentTime: 42,
			volume: 0.3,
		});
	});

	it("preserves an explicit collection source when replacing and appending tracks", async () => {
		const api = createApi();
		renderPlayback(api);
		await act(async () => {});
		const source = { kind: "playlist", playlistId: "p1", name: "Favorites" };
		await act(async () =>
			screen.getByRole("button", { name: "Play playlist" }).click(),
		);
		expect(api.replaceQueue).toHaveBeenCalledWith(["track-2"], "1", source);
		await act(async () =>
			screen.getByRole("button", { name: "Queue playlist" }).click(),
		);
		expect(api.appendQueueItem).toHaveBeenCalledWith(
			track.id,
			expect.any(String),
			source,
		);
		expect(api.appendQueueItem).toHaveBeenCalledWith(
			track2.id,
			expect.any(String),
			source,
		);
	});

	it.each(["Add to queue", "Play next"])(
		"marks individual %s as user sourced",
		async (action) => {
			const api = createApi();
			renderPlayback(api);
			await act(async () => {});
			await act(async () =>
				screen.getByRole("button", { name: action }).click(),
			);
			expect(api.appendQueueItem).toHaveBeenCalledWith(track.id, "1", {
				kind: "user",
			});
		},
	);

	it("refetches and retries unambiguous append intent once", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: queueItems, revision: "2" });
		vi.mocked(api.appendQueueItem)
			.mockRejectedValueOnce(queueConflict("2"))
			.mockResolvedValueOnce({ items: queueItems, revision: "3" });
		renderPlayback(api);
		await act(async () => {});

		await act(async () =>
			screen.getByRole("button", { name: "Append" }).click(),
		);

		expect(api.appendQueueItem).toHaveBeenNthCalledWith(1, "track-2", "1", {
			kind: "user",
		});
		expect(api.appendQueueItem).toHaveBeenNthCalledWith(2, "track-2", "2", {
			kind: "user",
		});
		expect(api.getQueue).toHaveBeenCalledTimes(2);
	});

	it("refetches stale remove and treats already-removed intent as satisfied", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: [queueItems[1]], revision: "2" });
		vi.mocked(api.removeQueueItem).mockRejectedValueOnce(queueConflict("2"));
		renderPlayback(api);

		await act(async () =>
			screen.getByRole("button", { name: "Remove" }).click(),
		);

		expect(api.removeQueueItem).toHaveBeenCalledTimes(1);
		expect(api.getQueue).toHaveBeenCalledTimes(2);
	});

	it("refetches and retries remove intent once when item still exists", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: queueItems, revision: "2" });
		vi.mocked(api.removeQueueItem)
			.mockRejectedValueOnce(queueConflict("2"))
			.mockResolvedValueOnce({ items: [queueItems[1]], revision: "3" });
		renderPlayback(api);
		await act(async () => {});

		await act(async () =>
			screen.getByRole("button", { name: "Remove" }).click(),
		);

		expect(api.removeQueueItem).toHaveBeenNthCalledWith(1, "item-1", "1");
		expect(api.removeQueueItem).toHaveBeenNthCalledWith(2, "item-1", "2");
		expect(api.getQueue).toHaveBeenCalledTimes(2);
	});

	it("refetches and retries an explicit play-with-context replace once", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: queueItems, revision: "2" });
		vi.mocked(api.replaceQueue)
			.mockRejectedValueOnce(queueConflict("2"))
			.mockResolvedValueOnce({
				items: [
					{ id: "item-9", trackId: track2.id, position: 0, track: track2 },
				],
				revision: "3",
			});
		renderPlayback(api);
		await act(async () => {});

		await act(async () =>
			screen.getByRole("button", { name: "Play from album" }).click(),
		);

		expect(api.replaceQueue).toHaveBeenNthCalledWith(1, ["track-2"], "1");
		expect(api.replaceQueue).toHaveBeenNthCalledWith(2, ["track-2"], "2");
		expect(screen.getByTestId("queue").textContent).toBe("track-2");
		expect(screen.getByTestId("track").textContent).toBe("Track 2");
		expect(screen.getByTestId("playing").textContent).toBe("true");
		expect(screen.getByTestId("queue-conflict").textContent).toBe("");
	});

	it.each([
		["clear", "Clear"],
		["reorder", "Reorder"],
	] as const)(
		"does not auto-merge %s conflicts and exposes them",
		async (operation, buttonName) => {
			const api = createApi();
			vi.mocked(api.getQueue)
				.mockResolvedValueOnce({ items: queueItems, revision: "1" })
				.mockResolvedValueOnce({ items: queueItems, revision: "2" });
			if (operation === "clear") {
				vi.mocked(api.clearQueue).mockRejectedValueOnce(queueConflict("2"));
			} else {
				vi.mocked(api.reorderQueue).mockRejectedValueOnce(queueConflict("2"));
			}
			renderPlayback(api);

			await act(async () =>
				screen.getByRole("button", { name: buttonName }).click(),
			);

			const mutation =
				operation === "clear" ? api.clearQueue : api.reorderQueue;
			expect(mutation).toHaveBeenCalledTimes(1);
			expect(api.getQueue).toHaveBeenCalledTimes(2);
			expect(screen.getByTestId("queue-conflict").textContent).toContain(
				"changed",
			);
		},
	);

	it("keeps the latest server Queue after an ambiguous reorder conflict", async () => {
		const api = createApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: queueItems, revision: "1" })
			.mockResolvedValueOnce({ items: [queueItems[1]], revision: "2" });
		vi.mocked(api.reorderQueue).mockRejectedValueOnce(queueConflict("2"));
		renderPlayback(api);

		await act(async () =>
			screen.getByRole("button", { name: "Reorder" }).click(),
		);

		expect(screen.getByTestId("queue").textContent).toBe("track-2");
		expect(screen.getByTestId("queue-conflict").textContent).toContain(
			"Review order",
		);
	});

	it.each([
		["append retry", "Append", true],
		["remove retry", "Remove", true],
		["already-satisfied remove", "Remove", false],
	] as const)(
		"clears stale conflict after %s",
		async (_scenario, buttonName, shouldRetry) => {
			const api = createApi();
			const latestItems = shouldRetry ? queueItems : [queueItems[1]];
			vi.mocked(api.getQueue)
				.mockResolvedValueOnce({ items: queueItems, revision: "1" })
				.mockResolvedValueOnce({ items: queueItems, revision: "2" })
				.mockResolvedValueOnce({ items: latestItems, revision: "3" });
			vi.mocked(api.reorderQueue).mockRejectedValueOnce(queueConflict("2"));
			if (buttonName === "Append") {
				vi.mocked(api.appendQueueItem)
					.mockRejectedValueOnce(queueConflict("3"))
					.mockResolvedValueOnce({ items: queueItems, revision: "4" });
			} else {
				vi.mocked(api.removeQueueItem).mockRejectedValueOnce(
					queueConflict("3"),
				);
				if (shouldRetry) {
					vi.mocked(api.removeQueueItem).mockResolvedValueOnce({
						items: [queueItems[1]],
						revision: "4",
					});
				}
			}
			renderPlayback(api);
			await act(async () => {});

			await act(async () =>
				screen.getByRole("button", { name: "Reorder" }).click(),
			);
			expect(screen.getByTestId("queue-conflict").textContent).toContain(
				"changed",
			);

			await act(async () =>
				screen.getByRole("button", { name: buttonName }).click(),
			);

			expect(screen.getByTestId("queue-conflict").textContent).toBe("");
		},
	);
});

function queueConflict(revision: string) {
	return new ApiError(409, {
		error: "conflict",
		code: "queue_revision_conflict",
		message: "queue changed since supplied revision",
		queue: { items: queueItems, revision },
	});
}

async function notifyQueueEvent(api: PlaybackApi, event: QueueEvent) {
	if (!api.subscribeQueueEvents) {
		throw new Error("Queue event subscription is unavailable");
	}
	const subscribe = vi.mocked(api.subscribeQueueEvents);
	await act(async () => {
		subscribe.mock.calls[0]?.[0](event);
	});
}
