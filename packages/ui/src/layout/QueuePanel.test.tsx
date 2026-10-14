import {
	ApiError,
	type Queue,
	type QueueItem,
	type QueueItemSource,
	type Track,
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
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	type PlaybackApi,
	PlaybackProvider,
} from "../playback/PlaybackProvider";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import { defaultPreferences } from "../widgets/types";
import { LayoutProvider } from "./LayoutProvider";
import { QueuePanel } from "./QueuePanel";

const track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "opus",
	genres: [],
};

const track2 = { ...track, id: "track-2", title: "Track 2" };
const track3 = { ...track, id: "track-3", title: "Track 3" };

const threeItems = [
	{ id: "item-1", trackId: track.id, position: 0, track },
	{ id: "item-2", trackId: track2.id, position: 1, track: track2 },
	{ id: "item-3", trackId: track3.id, position: 2, track: track3 },
];

function createApi(
	items: QueueItem[] = [
		{ id: "item-1", trackId: track.id, position: 0, track },
	],
): PlaybackApi {
	return {
		getQueue: vi.fn(async () => ({ items, revision: "1" })),
		replaceQueue: vi.fn(async () => ({ items: [], revision: "2" })),
		reorderQueue: vi.fn(async () => ({ items: [], revision: "2" })),
		appendQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
		removeQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
		getStreamUrl: (trackId) => `/stream/${trackId}`,
		getAlbumCoverUrl: (albumId) => `/cover/${albumId}`,
		getRadioStationStreamUrl: (stationId) => `/radio/${stationId}`,
		getRadioCatalogPreviewStreamUrl: (stationUuid) =>
			`/radio/preview/${stationUuid}`,
		getRadioNowPlaying: vi.fn(async () => ({})),
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

describe("QueuePanel", () => {
	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
	});

	it.each([
		{ rowTop: 280, rowBottom: 340, expectedScroll: 40 },
		{ rowTop: 50, rowBottom: 110, expectedScroll: -50 },
		{ rowTop: 150, rowBottom: 210, expectedScroll: 0 },
	])(
		"scrolls only the queue viewport for row at $rowTop",
		async ({ rowTop, rowBottom, expectedScroll }) => {
			const engine = new InMemoryPlaybackEngine();
			const view = render(
				<LayoutProvider initialPreferences={defaultPreferences}>
					<PlaybackProvider api={createApi()} engine={engine}>
						<QueuePanel embedded />
					</PlaybackProvider>
				</LayoutProvider>,
			);
			await screen.findByText("Track 1");
			const viewport = view.container.querySelector(
				".overflow-y-auto",
			) as HTMLElement;
			const scrollBy = vi.fn();
			Object.defineProperty(viewport, "scrollBy", { value: scrollBy });
			const bounds = vi
				.spyOn(Element.prototype, "getBoundingClientRect")
				.mockImplementation(function (this: Element) {
					return {
						top: this.tagName === "LI" ? rowTop : 100,
						bottom: this.tagName === "LI" ? rowBottom : 300,
					} as DOMRect;
				});
			try {
				await act(async () =>
					engine.play({
						type: "track",
						track,
						playbackUrl: "/stream/track-1",
						queueItemId: "item-1",
					}),
				);
				if (expectedScroll === 0) expect(scrollBy).not.toHaveBeenCalled();
				else
					expect(scrollBy).toHaveBeenCalledWith({
						top: expectedScroll,
						behavior: "smooth",
					});
			} finally {
				bounds.mockRestore();
			}
		},
	);

	it("plays a track when left-clicking the queue row", async () => {
		const engine = new InMemoryPlaybackEngine();
		render(
			<LayoutProvider initialPreferences={defaultPreferences}>
				<PlaybackProvider api={createApi()} engine={engine}>
					<QueuePanel />
				</PlaybackProvider>
			</LayoutProvider>,
		);

		const title = await screen.findByText("Track 1");
		const row = title.closest("li");

		await act(async () => {
			fireEvent.click(row as HTMLElement);
		});

		expect(engine.getState().source).toMatchObject({
			type: "track",
			playbackUrl: "/stream/track-1",
		});
	});

	it("groups consecutive equal sources and restarts numbering without reordering an album split by a user", async () => {
		const playlistSource: QueueItemSource = {
			kind: "playlist",
			playlistId: "p1",
			name: "Favorites",
		};
		const items = [
			makeItem("a1", albumSource),
			makeItem("a2", { ...albumSource }),
			makeItem("user", { kind: "user" }),
			makeItem("a3", albumSource),
			makeItem("a4", albumSource),
			makeItem("p1", playlistSource),
			makeItem("p2", { ...playlistSource }),
		];
		renderPanel(createApi(items));
		await screen.findByText("a1");
		const groups = screen.getAllByRole("region");
		expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual([
			"Album",
			"Added by you",
			"Album",
			"Favorites",
		]);
		expect(
			groups.map((group) =>
				within(group)
					.getAllByRole("button")
					.map((row) => row.firstElementChild?.textContent),
			),
		).toEqual([["1", "2"], ["1"], ["1", "2"], ["1", "2"]]);
		expect(
			groups.flatMap((group) =>
				within(group)
					.getAllByRole("button")
					.map(
						(row) =>
							items.find((item) => within(row).queryByText(item.track.title))
								?.id,
					),
			),
		).toEqual(items.map((item) => item.id));
		expect(
			screen.getAllByRole("link", {
				name: "Album: From album · Artist · 2 tracks",
			}),
		).toHaveLength(2);
		expect(
			screen
				.getByRole("link", { name: "Favorites: From playlist · 2 tracks" })
				.getAttribute("href"),
		).toBe("/playlists/p1");
	});

	it("shows played and current row states and counts only upcoming duration", async () => {
		renderPanel(createApi(threeItems));
		await screen.findByText("Track 2");
		expect(screen.getByText("3 tracks · 6 min left")).toBeTruthy();
		await act(async () => fireEvent.click(screen.getByText("Track 2")));
		expect(screen.getByText("1 track · 2 min left")).toBeTruthy();
		expect(getRow("Track 1").getAttribute("data-queue-state")).toBe("played");
		expect(getRow("Track 2").getAttribute("aria-current")).toBe("true");
		expect(getRow("Track 3").getAttribute("data-queue-state")).toBe("upcoming");
		expect(screen.getAllByRole("region")).toHaveLength(1);
		await act(async () => fireEvent.click(getRow("Track 1")));
		expect(getRow("Track 1").getAttribute("aria-current")).toBe("true");
		expect(getRow("Track 2").getAttribute("data-queue-state")).toBe("upcoming");
		expect(screen.getByText("2 tracks · 4 min left")).toBeTruthy();
		await act(async () => fireEvent.click(getRow("Track 3")));
		expect(screen.getByText("0 tracks · 0 min left")).toBeTruthy();
	});

	it("selects the correct queue item when the same track occurs twice", async () => {
		const items = [
			threeItems[0],
			{ ...threeItems[0], id: "repeat", position: 1 },
			threeItems[2],
		];
		const { engine, container } = renderPanel(createApi(items));
		await screen.findByText("Track 3");
		const rows = screen
			.getAllByText("Track 1")
			.map((title) => title.closest("li") as HTMLElement);
		await act(async () => fireEvent.click(rows[1]));
		expect(engine.getState().source).toMatchObject({
			queueItemId: "repeat",
			track: { id: track.id },
		});
		expect(container.querySelectorAll('[aria-current="true"]')).toHaveLength(1);
		expect(rows[1].getAttribute("aria-current")).toBe("true");
		expect(rows[0].getAttribute("data-queue-state")).toBe("played");
		expect(screen.getByText("1 track · 2 min left")).toBeTruthy();
	});

	it("removes only the clicked suggestion run, sequentially using each returned revision", async () => {
		const source: QueueItemSource = { kind: "suggestion", basedOn: [track.id] };
		const items = [
			threeItems[0],
			makeItem("s1", source),
			makeItem("s2", source),
			makeItem("user", { kind: "user" }),
			makeItem("s3", source),
		];
		const api = createApi(items);
		const firstRemoval = deferred<Queue>();
		vi.mocked(api.removeQueueItem)
			.mockImplementationOnce(() => firstRemoval.promise)
			.mockResolvedValueOnce({
				items: [items[0], items[3], items[4]],
				revision: "3",
			});
		renderPanel(api);
		await screen.findByText("s1");
		const groups = screen.getAllByRole("region", { name: "For you" });
		expect(
			within(groups[0]).getAllByTitle("Based on Track 1").length,
		).toBeGreaterThan(0);
		await act(async () =>
			fireEvent.click(
				within(groups[0]).getByRole("button", { name: "Not now" }),
			),
		);
		expect(api.removeQueueItem).toHaveBeenCalledExactlyOnceWith("s1", "1");
		expect(
			(
				within(groups[1]).getByRole("button", {
					name: "Not now",
				}) as HTMLButtonElement
			).disabled,
		).toBe(true);
		await act(async () =>
			firstRemoval.resolve({
				items: items.filter((item) => item.id !== "s1"),
				revision: "2",
			}),
		);
		expect(api.removeQueueItem).toHaveBeenNthCalledWith(2, "s2", "2");
		expect(api.removeQueueItem).toHaveBeenCalledTimes(2);
		expect(screen.queryByText("s1")).toBeNull();
		expect(screen.queryByText("s2")).toBeNull();
		expect(screen.getByText("s3")).toBeTruthy();
		expect(screen.getByText("user")).toBeTruthy();
		expect(screen.getByText("Track 1")).toBeTruthy();
	});

	it("loads missing suggestion seed titles from the library without adding them to the queue", async () => {
		const source: QueueItemSource = {
			kind: "suggestion",
			basedOn: [track.id, "external-seed"],
		};
		const api = createApi([threeItems[0], makeItem("suggestion", source)]);
		const seedResponse = deferred<Track>();
		api.getTrack = vi.fn(() => seedResponse.promise);
		renderPanel(api);
		await screen.findByText("suggestion");
		const group = screen.getByRole("region", { name: "For you" });
		expect(
			within(group).getAllByTitle("Based on Track 1, Loading track…").length,
		).toBeGreaterThan(0);
		expect(api.getTrack).toHaveBeenCalledExactlyOnceWith("external-seed");

		await act(async () =>
			seedResponse.resolve({
				...track,
				id: "external-seed",
				title: "Library seed title",
			}),
		);

		expect(
			within(group).getAllByTitle("Based on Track 1, Library seed title")
				.length,
		).toBeGreaterThan(0);
		expect(within(group).queryAllByTitle(/Loading track/)).toHaveLength(0);
		expect(screen.queryByText("Library seed title")).toBeNull();
		expect(screen.getByText("2 tracks · 4 min left")).toBeTruthy();
		expect(api.getTrack).toHaveBeenCalledTimes(1);
		expect(api.appendQueueItem).not.toHaveBeenCalled();
		expect(api.replaceQueue).not.toHaveBeenCalled();
	});

	it("retries a failed external seed lookup only after coming online and restores its title", async () => {
		const api = createApi([
			makeItem("suggestion", {
				kind: "suggestion",
				basedOn: [track.id, "external-seed"],
			}),
			threeItems[0],
		]);
		const error = new Error("Network unavailable");
		api.getTrack = vi
			.fn()
			.mockRejectedValueOnce(error)
			.mockResolvedValue({
				...track,
				id: "external-seed",
				title: "Recovered seed title",
			});
		const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
		renderPanel(api);
		await screen.findByText("suggestion");
		const group = screen.getByRole("region", { name: "For you" });
		await waitFor(() =>
			expect(
				within(group).getAllByTitle("Based on Track 1, Unavailable track")
					.length,
			).toBeGreaterThan(0),
		);
		await act(async () => {});
		expect(api.getTrack).toHaveBeenCalledExactlyOnceWith("external-seed");
		expect(warning).toHaveBeenCalledWith(
			"Failed to load queue suggestion source track",
			{
				trackId: "external-seed",
				error,
			},
		);
		expect(within(group).queryAllByTitle(/Loading track/)).toHaveLength(0);
		expect(within(group).getByRole("button", { name: "Not now" })).toBeTruthy();
		await act(async () => window.dispatchEvent(new Event("online")));
		expect(api.getTrack).toHaveBeenNthCalledWith(2, "external-seed");
		expect(api.getTrack).toHaveBeenCalledTimes(2);
		expect(
			within(group).getAllByTitle("Based on Track 1, Recovered seed title")
				.length,
		).toBeGreaterThan(0);
		expect(
			within(group).queryAllByTitle(/Unavailable track|Loading track/),
		).toHaveLength(0);
		await act(async () => window.dispatchEvent(new Event("online")));
		expect(api.getTrack).toHaveBeenCalledTimes(2);
	});

	it("shows unavailable seed metadata when a legacy host has no track lookup", async () => {
		renderPanel(
			createApi([
				makeItem("suggestion", {
					kind: "suggestion",
					basedOn: ["external-seed"],
				}),
			]),
		);
		await screen.findByText("suggestion");
		const group = screen.getByRole("region", { name: "For you" });
		await waitFor(() =>
			expect(
				within(group).getAllByTitle("Based on Unavailable track").length,
			).toBeGreaterThan(0),
		);
		expect(within(group).queryAllByTitle(/Loading track/)).toHaveLength(0);
	});

	it("groups structurally equal suggestion sources but keeps reversed seed arrays separate", async () => {
		const api = createApi([
			threeItems[0],
			threeItems[1],
			makeItem("s1", { kind: "suggestion", basedOn: [track.id, track2.id] }),
			makeItem("s2", { kind: "suggestion", basedOn: [track.id, track2.id] }),
			makeItem("s3", { kind: "suggestion", basedOn: [track2.id, track.id] }),
		]);
		api.getTrack = vi.fn();
		renderPanel(api);
		await screen.findByText("s3");
		const groups = screen.getAllByRole("region", { name: "For you" });
		expect(groups).toHaveLength(2);
		expect(within(groups[0]).getByText("s1")).toBeTruthy();
		expect(within(groups[0]).getByText("s2")).toBeTruthy();
		expect(within(groups[0]).queryByText("s3")).toBeNull();
		expect(
			within(groups[0]).getAllByTitle("Based on Track 1, Track 2").length,
		).toBeGreaterThan(0);
		expect(within(groups[1]).getByText("s3")).toBeTruthy();
		expect(
			within(groups[1]).getAllByTitle("Based on Track 2, Track 1").length,
		).toBeGreaterThan(0);
		expect(api.getTrack).not.toHaveBeenCalled();
	});

	it("keeps the header and disables Clear for an empty queue", async () => {
		const api = createApi([]);
		renderPanel(api);
		await act(async () => {});
		expect(screen.getByRole("heading", { name: "Queue" })).toBeTruthy();
		expect(screen.getByText("Queue is empty")).toBeTruthy();
		expect(screen.getByText("0 tracks · 0 min left")).toBeTruthy();
		const clear = screen.getByRole("button", {
			name: "Clear",
		}) as HTMLButtonElement;
		expect(clear.disabled).toBe(true);
		fireEvent.click(clear);
		expect(api.replaceQueue).not.toHaveBeenCalled();
	});

	it("clears with the current revision and renders the empty state", async () => {
		const api = createApi(threeItems);
		renderPanel(api);
		await screen.findByText("Track 1");
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Clear" })),
		);
		expect(api.replaceQueue).toHaveBeenCalledExactlyOnceWith([], "1");
		expect(screen.getByText("Queue is empty")).toBeTruthy();
	});

	it("shows a failed Clear and allows a successful retry", async () => {
		const api = createApi(threeItems);
		vi.mocked(api.replaceQueue).mockRejectedValueOnce(new Error("offline"));
		const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
		renderPanel(api);
		await screen.findByText("Track 1");
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Clear" })),
		);
		expect(screen.getByRole("alert").textContent).toBe("Failed to clear queue");
		expect(screen.getByText("Track 1")).toBeTruthy();
		expect(warning).toHaveBeenCalledWith("Failed to clear queue", {
			error: expect.any(Error),
		});
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Clear" })),
		);
		expect(screen.queryByRole("alert")).toBeNull();
		expect(screen.getByText("Queue is empty")).toBeTruthy();
	});

	it("surfaces a Clear revision conflict and preserves the refreshed queue", async () => {
		const api = createApi(threeItems);
		vi.mocked(api.replaceQueue).mockRejectedValueOnce(
			new ApiError(409, {
				error: "Conflict",
				code: "queue_revision_conflict",
				message: "Queue changed",
			}),
		);
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce({ items: threeItems, revision: "1" })
			.mockResolvedValueOnce({ items: [threeItems[2]], revision: "2" });
		renderPanel(api);
		await screen.findByText("Track 1");
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Clear" })),
		);
		expect(screen.getByRole("alert").textContent).toContain(
			"Queue changed in another Playback Client",
		);
		expect(screen.getByText("Track 3")).toBeTruthy();
		expect(screen.queryByText("Track 1")).toBeNull();
		expect(api.replaceQueue).toHaveBeenCalledTimes(1);
	});

	it("stops suggestion dismissal on failure and reports the error", async () => {
		const source: QueueItemSource = { kind: "suggestion", basedOn: [] };
		const api = createApi([makeItem("s1", source), makeItem("s2", source)]);
		vi.mocked(api.removeQueueItem).mockRejectedValueOnce(new Error("offline"));
		vi.spyOn(console, "warn").mockImplementation(() => {});
		renderPanel(api);
		await screen.findByText("s1");
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Not now" })),
		);
		expect(screen.getByRole("alert").textContent).toBe(
			"Failed to remove queue suggestions",
		);
		expect(api.removeQueueItem).toHaveBeenCalledExactlyOnceWith("s1", "1");
		expect(screen.getByText("s2")).toBeTruthy();
	});

	it.each(["Enter", " "])("plays a focused queue row with %j", async (key) => {
		const { engine } = renderPanel(createApi(threeItems));
		await screen.findByText("Track 2");
		const row = getRow("Track 2");
		expect(row.tabIndex).toBe(0);
		row.focus();
		await act(async () => fireEvent.keyDown(row, { key }));
		expect(engine.getState().source).toMatchObject({ queueItemId: "item-2" });
		expect(row.getAttribute("aria-current")).toBe("true");
	});

	it("ignores unrelated keys", async () => {
		const { engine } = renderPanel(createApi());
		await screen.findByText("Track 1");
		const play = vi.spyOn(engine, "play");
		fireEvent.keyDown(getRow("Track 1"), { key: "Escape" });
		expect(play).not.toHaveBeenCalled();
	});
});

const albumSource: QueueItemSource = {
	kind: "album",
	albumId: "album-1",
	albumTitle: "Album",
	artistName: "Artist",
};

function makeItem(id: string, source: QueueItemSource): QueueItem {
	return {
		id,
		trackId: id,
		position: 0,
		track: { ...track, id, title: id },
		source,
	};
}

function renderPanel(api: PlaybackApi) {
	const engine = new InMemoryPlaybackEngine();
	const view = render(
		<LayoutProvider initialPreferences={defaultPreferences}>
			<PlaybackProvider api={api} engine={engine}>
				<QueuePanel embedded />
			</PlaybackProvider>
		</LayoutProvider>,
	);
	return { ...view, engine };
}

function getRow(title: string): HTMLElement {
	return screen.getByText(title).closest("li") as HTMLElement;
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	const promise = new Promise<T>((complete) => {
		resolve = complete;
	});
	return { promise, resolve };
}
