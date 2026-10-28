import type { ExternalTrack, LibrarySearchResponse } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
	within,
} from "@testing-library/react";
import { useState } from "react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { invalidatePlaylistCache } from "#/lib/playlist-query-cache";
import { LibrarySearchDialog } from "./library-search-dialog";

const mocks = vi.hoisted(() => ({
	searchLibrary: vi.fn(),
	searchExternalSource: vi.fn(),
	hasExternalSource: false,
	navigate: vi.fn(),
	playTrack: vi.fn(),
	playExternalTrack: vi.fn(),
	addToQueue: vi.fn(),
	toastSuccess: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		searchLibrary: mocks.searchLibrary,
		searchExternalSource: mocks.searchExternalSource,
		getAlbumCoverUrl: (id: string) => `/api/v1/library/albums/${id}/cover`,
	},
}));
vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: (capability: string) =>
		capability === "external-source.v1" && mocks.hasExternalSource,
}));
vi.mock("@tanstack/react-router", () => ({
	useNavigate: () => mocks.navigate,
}));
vi.mock("@repo/ui", async (importOriginal) => ({
	...(await importOriginal<typeof import("@repo/ui")>()),
	usePlayback: () => ({
		playTrack: mocks.playTrack,
		addToQueue: mocks.addToQueue,
		playExternalTrack: mocks.playExternalTrack,
	}),
	toast: { success: mocks.toastSuccess, error: mocks.toastError },
}));

const nemo = {
	type: "track" as const,
	id: "track-nemo",
	name: "Nemo",
	match: "direct" as const,
	artists: [{ id: "artist-1", name: "Nightwish", role: "main" }],
	album: { id: "album-decades", name: "Decades" },
};
const empty: LibrarySearchResponse = {
	tracks: [],
	albums: [],
	artists: [],
	genres: [],
	playlists: [],
};
const results: LibrarySearchResponse = {
	...empty,
	bestMatch: nemo,
	albums: [
		{
			type: "album",
			id: "album-decades",
			name: "Decades",
			match: "related",
			artists: nemo.artists,
		},
	],
	artists: [
		{ type: "artist", id: "artist-1", name: "Nightwish", match: "related" },
	],
	playlists: [
		{ type: "playlist", id: "pl-1", name: "Nemo mix", match: "direct" },
	],
};

function renderDialog(onOpenChange = vi.fn()) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return {
		client,
		onOpenChange,
		...render(
			<QueryClientProvider client={client}>
				<LibrarySearchDialog open onOpenChange={onOpenChange} />
			</QueryClientProvider>,
		),
	};
}

describe("LibrarySearchDialog", () => {
	beforeEach(() => {
		vi.useFakeTimers({ shouldAdvanceTime: true });
		mocks.searchLibrary.mockResolvedValue(results);
		mocks.addToQueue.mockResolvedValue(undefined);
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
		vi.useRealTimers();
		mocks.hasExternalSource = false;
	});

	describe("External Source", () => {
		const daftPunk: ExternalTrack = {
			id: "token.signature",
			title: "Around the World",
			artistName: "Daft Punk",
			durationMs: 429_000,
			webpageUrl: "https://soundcloud.com/daftpunk/around-the-world",
			extractor: "Soundcloud",
		};

		beforeEach(() => {
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk],
			});
			mocks.playExternalTrack.mockResolvedValue(undefined);
		});

		it("stays hidden and never searches without the capability", async () => {
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByText("Nemo mix");
			expect(screen.queryByRole("group", { name: "External" })).toBeNull();
			expect(mocks.searchExternalSource).not.toHaveBeenCalled();
		});

		it("lists External Tracks in their own group after the library groups", async () => {
			mocks.hasExternalSource = true;
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "External" });
			const option = await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			expect(option.textContent).toContain("Daft Punk");
			expect(option.textContent).toContain("7:09");
			expect(option.textContent).not.toContain("Open");
			expect(mocks.searchExternalSource).toHaveBeenCalledWith("ne");
			const library = screen.getByRole("region", { name: "From Your Library" });
			expect(within(library).queryByText("Around the World")).toBeNull();
			expect(
				library.compareDocumentPosition(external) &
					Node.DOCUMENT_POSITION_FOLLOWING,
			).toBeTruthy();
			// No queue action: an External Track never joins the Queue.
			expect(
				within(external).queryByRole("button", { name: /to queue/ }),
			).toBeNull();
		});

		it("plays the chosen External Track and closes", async () => {
			mocks.hasExternalSource = true;
			const { onOpenChange } = renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			fireEvent.click(
				await screen.findByRole("option", { name: /^Around the World/ }),
			);
			expect(mocks.playExternalTrack).toHaveBeenCalledWith(daftPunk);
			expect(mocks.playTrack).not.toHaveBeenCalled();
			expect(onOpenChange).toHaveBeenCalledWith(false);
		});

		it("reaches External Tracks with the keyboard after the library rows", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo] });
			const { onOpenChange } = renderDialog();
			const input = screen.getByRole("combobox");
			fireEvent.change(input, { target: { value: "ne" } });
			await screen.findByRole("option", { name: /^Around the World/ });
			fireEvent.keyDown(input, { key: "ArrowDown" });
			fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
			expect(mocks.addToQueue).not.toHaveBeenCalled();
			fireEvent.keyDown(input, { key: "Enter" });
			expect(mocks.playExternalTrack).toHaveBeenCalledWith(daftPunk);
			expect(onOpenChange).toHaveBeenCalledWith(false);
		});

		it("keeps the library groups when the External Source fails", async () => {
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockRejectedValue(
				new Error("external_source_failed"),
			);
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "External" });
			expect(
				(await within(external).findByRole("alert")).textContent,
			).toContain("External results could not be loaded.");
			expect(screen.getAllByRole("alert")).toHaveLength(1);
			const library = screen.getByRole("region", { name: "From Your Library" });
			expect(within(library).getByText("Nemo mix")).toBeTruthy();

			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk],
			});
			fireEvent.click(within(external).getByRole("button", { name: "Retry" }));
			await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			expect(within(external).queryByRole("alert")).toBeNull();
		});

		it("shows its own loading and empty states", async () => {
			mocks.hasExternalSource = true;
			let finish!: (value: { source: string; items: ExternalTrack[] }) => void;
			mocks.searchExternalSource.mockImplementation(
				() =>
					new Promise((resolve) => {
						finish = resolve;
					}),
			);
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "External" });
			await screen.findByText("Nemo mix");
			expect(within(external).getByText("Searching…")).toBeTruthy();
			await act(async () => finish({ source: "scsearch", items: [] }));
			expect(
				await within(external).findByText("No External results for “ne”."),
			).toBeTruthy();
		});
	});

	it("shows Best Match before ordered nonempty categories", async () => {
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "ne" },
		});

		const section = await screen.findByRole("region", {
			name: "From Your Library",
		});
		// The coherent response preserves server ordering.
		await within(section).findByText("Nemo mix");
		const labels = within(section)
			.getAllByText(/^(Best Match|Track|Album|Artist|Genre|Playlist)$/)
			.map((node) => node.textContent);
		expect(labels).toEqual(["Best Match", "Album", "Artist", "Playlist"]);
		expect(within(section).getByText("Nemo")).toBeTruthy();
		expect(within(section).getByText("Decades")).toBeTruthy();
		expect(within(section).getByText("Nemo mix")).toBeTruthy();
		expect(within(section).queryByText("Chill")).toBeNull();
		expect(mocks.searchLibrary).toHaveBeenCalledWith("ne");
	});

	it("does not invent related Albums or Artists from a matching Track", async () => {
		mocks.searchLibrary.mockResolvedValue({
			...empty,
			bestMatch: nemo,
			tracks: [nemo],
		});
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "nemo" },
		});
		await screen.findByRole("group", { name: "Track" });
		expect(screen.queryByRole("group", { name: "Album" })).toBeNull();
		expect(screen.queryByRole("group", { name: "Artist" })).toBeNull();
	});

	it("shows server Genre results without scanning Tracks", async () => {
		mocks.searchLibrary.mockResolvedValue({
			...empty,
			genres: [
				{
					type: "genre",
					id: "g-metal",
					name: "Symphonic Metal",
					match: "direct",
				},
			],
		});
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "symph" },
		});
		expect(await screen.findByText("Symphonic Metal")).toBeTruthy();
		expect(screen.getByText("Genre")).toBeTruthy();
	});

	it("plays a track on Enter and opens an album from its row", async () => {
		const { onOpenChange } = renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		await screen.findByText("Nemo");
		await screen.findByRole("option", { name: /^Decades/ });

		fireEvent.keyDown(input, { key: "Enter" });
		expect(mocks.playTrack).toHaveBeenCalledWith("track-nemo");
		expect(onOpenChange).toHaveBeenCalledWith(false);

		const decades = screen
			.getByText("Decades", { selector: "span" })
			.closest("button");
		expect(decades).toBeTruthy();
		if (decades) fireEvent.click(decades);
		expect(mocks.navigate).toHaveBeenCalledWith({
			to: "/library/$albumId",
			params: { albumId: "album-decades" },
		});
	});

	it("moves the active row with the arrow keys", async () => {
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		await screen.findByText("Nemo");
		await screen.findByRole("option", { name: /^Decades/ });

		const optionFor = (title: string) =>
			screen.getByText(title, { selector: "span" }).closest("button");
		expect(optionFor("Nemo")?.getAttribute("aria-selected")).toBe("true");
		fireEvent.keyDown(input, { key: "ArrowDown" });
		expect(optionFor("Decades")?.getAttribute("aria-selected")).toBe("true");
		fireEvent.keyDown(input, { key: "Enter" });
		expect(mocks.navigate).toHaveBeenCalledWith(
			expect.objectContaining({ to: "/library/$albumId" }),
		);
	});

	it("shows covers, a unique total and a footer outside the scrolling list", async () => {
		mocks.searchLibrary.mockResolvedValue({
			...results,
			tracks: [nemo],
			total: 42,
		});
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		const count = await screen.findByRole("status");
		expect(count.textContent).toBe("42 results");
		const track = within(
			screen.getByRole("group", { name: "Track" }),
		).getByRole("option");
		expect(track.querySelector("img")?.getAttribute("src")).toBe(
			"/api/v1/library/albums/album-decades/cover",
		);
		const footer = screen.getByRole("button", { name: /See all results/ });
		expect(screen.getByRole("listbox").contains(footer)).toBe(false);
		expect(
			screen.getByRole("option", { selected: true }).parentElement?.className,
		).toContain("bg-muted");
	});

	it("counts Best Match once on older servers without totals", async () => {
		mocks.searchLibrary.mockResolvedValue({
			...empty,
			bestMatch: nemo,
			tracks: [nemo],
		});
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		expect((await screen.findByRole("status")).textContent).toBe("1 result");
	});

	it("adds a track without playing or closing, blocks duplicate pending adds", async () => {
		let finish!: () => void;
		mocks.addToQueue.mockImplementationOnce(
			() =>
				new Promise<void>((resolve) => {
					finish = resolve;
				}),
		);
		const { onOpenChange } = renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		const add = await screen.findByRole("button", {
			name: "Add Nemo to queue",
		});
		expect(add.closest("[role=option]")).toBeNull();
		fireEvent.click(add);
		fireEvent.click(add);
		fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
		expect(mocks.addToQueue).toHaveBeenCalledExactlyOnceWith("track-nemo");
		expect(add.getAttribute("aria-disabled")).toBe("true");
		expect(mocks.playTrack).not.toHaveBeenCalled();
		expect(onOpenChange).not.toHaveBeenCalled();
		await act(async () => finish());
		expect(mocks.toastSuccess).toHaveBeenCalledWith("Added “Nemo” to queue");
		fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
		await waitFor(() => expect(mocks.addToQueue).toHaveBeenCalledTimes(2));
	});

	it("reports queue errors and allows retry", async () => {
		mocks.addToQueue.mockRejectedValueOnce(new Error("Queue unavailable"));
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		const add = await screen.findByRole("button", {
			name: "Add Nemo to queue",
		});
		fireEvent.click(add);
		await waitFor(() =>
			expect(mocks.toastError).toHaveBeenCalledWith(
				"Failed to add track to queue",
			),
		);
		expect(add.getAttribute("aria-disabled")).toBe("false");
		fireEvent.click(add);
		await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
		warn.mockRestore();
	});

	it("expands all results and resets the limit when the query changes", async () => {
		const tracks = Array.from({ length: 8 }, (_, i) => ({
			...nemo,
			id: `track-${i}`,
			name: `Track ${i}`,
		}));
		mocks.searchLibrary.mockImplementation(
			async (_q: string, all?: boolean) => ({
				...empty,
				total: 8,
				tracks: all ? tracks : tracks.slice(0, 5),
			}),
		);
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "track" } });
		await screen.findByText("Track 0");
		expect(screen.getAllByRole("option")).toHaveLength(5);
		fireEvent.click(screen.getByRole("button", { name: /See all results/ }));
		await screen.findByText("Track 7");
		expect(mocks.searchLibrary).toHaveBeenCalledWith("track", true);
		expect(screen.getAllByRole("option")).toHaveLength(8);
		expect(document.activeElement).toBe(input);
		fireEvent.change(input, { target: { value: "next" } });
		await waitFor(() =>
			expect(mocks.searchLibrary).toHaveBeenLastCalledWith("next"),
		);
		expect(
			screen.queryByRole("button", { name: "Show top results" }),
		).toBeNull();
	});

	it("tells the user when nothing matches", async () => {
		mocks.searchLibrary.mockResolvedValue(empty);
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "zzz" },
		});
		expect(await screen.findByText(/matches “zzz”/)).toBeTruthy();
	});

	it("shows failures instead of empty results and retries the search", async () => {
		mocks.searchLibrary.mockRejectedValue(
			new Error("Track search unavailable"),
		);
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		expect(await screen.findByRole("alert")).toHaveProperty(
			"textContent",
			"Search results could not be loaded.Retry",
		);
		expect(screen.queryByText(/Nothing in your library/)).toBeNull();
		mocks.searchLibrary.mockResolvedValue(results);
		fireEvent.click(screen.getByRole("button", { name: "Retry" }));
		await screen.findByText("Nemo");
		await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
	});

	it("hides stale options and blocks Enter until the edited query settles", async () => {
		mocks.searchLibrary.mockImplementation(async (q: string) =>
			q === "ne" ? results : empty,
		);
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		await screen.findByText("Nemo");
		fireEvent.change(input, { target: { value: "zzz" } });
		expect(screen.queryAllByRole("option")).toHaveLength(0);
		fireEvent.keyDown(input, { key: "Enter" });
		expect(mocks.playTrack).not.toHaveBeenCalled();
		expect(await screen.findByText(/matches “zzz”/)).toBeTruthy();
	});

	it.each(["track", "album", "artist", "genre", "playlist"] as const)(
		"highlights a server-selected %s and keeps its category occurrence within five rows",
		async (type) => {
			const best = {
				type,
				id: "best",
				name: "Exact name",
				match: "direct" as const,
			};
			const rows = Array.from({ length: 7 }, (_, i) => ({
				...best,
				id: `row-${i}`,
				name: `Other ${i}`,
			}));
			mocks.searchLibrary.mockResolvedValue({
				...empty,
				bestMatch: best,
				[`${type}s`]: [best, ...rows],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "Exact name" },
			});
			const highlight = await screen.findByRole("group", {
				name: "Best Match",
			});
			expect(
				within(highlight).getByRole("option", { name: "Exact name" }),
			).toBeTruthy();
			const occurrences = screen.getAllByRole("option", { name: "Exact name" });
			expect(occurrences).toHaveLength(2);
			const options = screen.getAllByRole("option");
			expect(options).toHaveLength(6);
			expect(new Set(options.map((option) => option.id)).size).toBe(6);
			expect(options[1]).toBe(occurrences[1]);
			expect(screen.queryByText("Other 4")).toBeNull();
			const input = screen.getByRole("combobox");
			expect(input.getAttribute("aria-activedescendant")).toBe(
				occurrences[0].id,
			);
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(input.getAttribute("aria-activedescendant")).toBe(
				occurrences[1].id,
			);
			expect(screen.getAllByRole("option", { selected: true })).toEqual([
				occurrences[1],
			]);
		},
	);

	it.each(["corrected", "related"] as const)(
		"does not invent Best Match for %s results",
		async (match) => {
			mocks.searchLibrary.mockResolvedValue({
				...empty,
				tracks: [{ ...nemo, match }],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "nmeo" },
			});
			await screen.findByText("Nemo");
			expect(screen.queryByRole("group", { name: "Best Match" })).toBeNull();
		},
	);

	it.each([
		[
			"artist",
			{ to: "/library/artists/$artistId", params: { artistId: "selected" } },
		],
		["genre", { to: "/library/genres/$genre", params: { genre: "İzmir" } }],
		[
			"playlist",
			{ to: "/playlists/$playlistId", params: { playlistId: "selected" } },
		],
	] as const)(
		"navigates the selected %s using original metadata",
		async (type, destination) => {
			mocks.searchLibrary.mockResolvedValue({
				...empty,
				bestMatch: { type, id: "selected", name: "İzmir", match: "direct" },
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "izmir" },
			});
			fireEvent.click(await screen.findByRole("option", { name: "İzmir" }));
			expect(mocks.navigate).toHaveBeenCalledWith(destination);
		},
	);

	it("waits 200 ms, ignores punctuation, and searches one-character names", async () => {
		vi.useFakeTimers({ shouldAdvanceTime: false });
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "... / —" } });
		await act(() => vi.advanceTimersByTimeAsync(250));
		expect(mocks.searchLibrary).not.toHaveBeenCalled();
		fireEvent.change(input, { target: { value: "U" } });
		expect(screen.getByText("Searching…")).toBeTruthy();
		await act(() => vi.advanceTimersByTimeAsync(199));
		expect(mocks.searchLibrary).not.toHaveBeenCalled();
		await act(() => vi.advanceTimersByTimeAsync(1));
		expect(mocks.searchLibrary).toHaveBeenCalledWith("U");
	});

	it("ignores a late older response and activates only the current result", async () => {
		let resolveOld!: (value: LibrarySearchResponse) => void;
		mocks.searchLibrary.mockImplementation((q: string) =>
			q === "old"
				? new Promise<LibrarySearchResponse>((resolve) => {
						resolveOld = resolve;
					})
				: Promise.resolve(results),
		);
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "old" } });
		await act(() => vi.advanceTimersByTimeAsync(210));
		expect(screen.getByText("Searching…")).toBeTruthy();
		fireEvent.change(input, { target: { value: "nemo" } });
		await screen.findByText("Nemo");
		await act(async () =>
			resolveOld({
				...empty,
				bestMatch: { ...nemo, id: "old", name: "Old result" },
			}),
		);
		expect(screen.queryByText("Old result")).toBeNull();
		fireEvent.keyDown(input, { key: "Enter" });
		expect(mocks.playTrack).toHaveBeenCalledWith("track-nemo");
	});

	it("preserves selected identity when current-query results reorder", async () => {
		const second = { ...nemo, id: "second", name: "Second" };
		mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo, second] });
		const { client } = renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		await screen.findByText("Nemo");
		fireEvent.keyDown(input, { key: "ArrowDown" });
		mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [second, nemo] });
		await act(async () => {
			await client.invalidateQueries({ queryKey: ["library", "search"] });
		});
		expect(
			screen
				.getByRole("option", { name: /^Second/ })
				.getAttribute("aria-selected"),
		).toBe("true");
		fireEvent.keyDown(input, { key: "Enter" });
		expect(mocks.playTrack).toHaveBeenCalledWith("second");
	});

	it("keeps visible current-query data and offers retry after a refresh failure", async () => {
		const { client } = renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		await screen.findByText("Nemo");
		mocks.searchLibrary.mockRejectedValue(new Error("Unavailable"));
		await act(async () => {
			await client.invalidateQueries({ queryKey: ["library", "search"] });
		});
		expect(await screen.findByRole("alert")).toHaveProperty(
			"textContent",
			"Search results could not be refreshed.Retry",
		);
		expect(screen.getByText("Nemo")).toBeTruthy();
		expect(screen.queryByText(/Nothing in your library/)).toBeNull();
	});

	it("refreshes cached search results after a Playlist mutation", async () => {
		const { client } = renderDialog();
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		await screen.findByText("Nemo mix");
		mocks.searchLibrary.mockResolvedValue({ ...results, playlists: [] });
		await act(async () => {
			await invalidatePlaylistCache(client, "pl-1");
		});
		await waitFor(() => expect(screen.queryByText("Nemo mix")).toBeNull());
	});

	it("starts empty after a rapid close and reopen", async () => {
		const client = new QueryClient({
			defaultOptions: { queries: { retry: false } },
		});
		const dialog = (open: boolean) => (
			<QueryClientProvider client={client}>
				<LibrarySearchDialog open={open} onOpenChange={vi.fn()} />
			</QueryClientProvider>
		);
		const { rerender } = render(dialog(true));
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "ne" } });
		await screen.findByText("Nemo");
		rerender(dialog(false));
		rerender(dialog(true));
		expect(screen.getByRole("combobox")).toHaveProperty("value", "");
		expect(screen.queryAllByRole("option")).toHaveLength(0);
		fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
		expect(mocks.playTrack).not.toHaveBeenCalled();
	});

	it("scrolls the listbox viewport to reveal keyboard selections without moving the page", async () => {
		renderDialog();
		const input = screen.getByRole("combobox");
		fireEvent.change(input, { target: { value: "ne" } });
		await screen.findByText("Nemo mix");
		const listbox = screen.getByRole("listbox");
		const album = screen.getByRole("option", { name: /^Decades/ });
		const track = screen.getByRole("option", { name: /^Nemo\s*Nightwish/ });
		vi.spyOn(listbox, "getBoundingClientRect").mockReturnValue({
			top: 100,
			bottom: 200,
		} as DOMRect);
		vi.spyOn(album, "getBoundingClientRect").mockReturnValue({
			top: 220,
			bottom: 260,
		} as DOMRect);
		vi.spyOn(track, "getBoundingClientRect").mockReturnValue({
			top: 40,
			bottom: 80,
		} as DOMRect);
		document.documentElement.scrollTop = 25;
		fireEvent.keyDown(input, { key: "ArrowDown" });
		expect(listbox.scrollTop).toBe(60);
		fireEvent.keyDown(input, { key: "ArrowUp" });
		expect(listbox.scrollTop).toBe(0);
		expect(document.documentElement.scrollTop).toBe(25);
		document.documentElement.scrollTop = 0;
	});

	it.each(["button", "shortcut"])(
		"restores focus to the %s opener after Escape",
		async (opener) => {
			function SearchHarness() {
				const [open, setOpen] = useState(false);
				return (
					<>
						<button type="button" onClick={() => setOpen(true)}>
							Open search
						</button>
						<LibrarySearchDialog open={open} onOpenChange={setOpen} />
					</>
				);
			}
			const client = new QueryClient();
			render(
				<QueryClientProvider client={client}>
					<SearchHarness />
				</QueryClientProvider>,
			);
			const button = screen.getByRole("button", { name: "Open search" });
			button.focus();
			if (opener === "button") fireEvent.click(button);
			else fireEvent.keyDown(button, { key: "k", ctrlKey: true });
			const input = screen.getByRole("combobox");
			// Base UI moves focus into the dialog on the next frame.
			await act(async () => {
				await vi.advanceTimersByTimeAsync(50);
			});
			expect(document.activeElement).toBe(input);
			fireEvent.keyDown(input, { key: "Escape" });
			await act(async () => {
				await vi.advanceTimersByTimeAsync(1);
			});
			await waitFor(() => expect(document.activeElement).toBe(button));
		},
	);

	it("opens with Ctrl+K or / and ignores / while typing elsewhere", () => {
		const onOpenChange = vi.fn();
		const client = new QueryClient();
		render(
			<QueryClientProvider client={client}>
				<LibrarySearchDialog open={false} onOpenChange={onOpenChange} />
				<input aria-label="Other field" />
			</QueryClientProvider>,
		);
		fireEvent.keyDown(window, { key: "k", ctrlKey: true });
		expect(onOpenChange).toHaveBeenLastCalledWith(true);
		fireEvent.keyDown(document.body, { key: "/" });
		expect(onOpenChange).toHaveBeenCalledTimes(2);
		fireEvent.keyDown(screen.getByLabelText("Other field"), { key: "/" });
		expect(onOpenChange).toHaveBeenCalledTimes(2);
	});
});
