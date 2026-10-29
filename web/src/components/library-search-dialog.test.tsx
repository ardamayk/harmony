import {
	ApiError,
	type ExternalTrack,
	type LibrarySearchResponse,
} from "@repo/api-client";
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
import { SoulseekInstallContext } from "./soulseek-install-context";

const mocks = vi.hoisted(() => ({
	searchLibrary: vi.fn(),
	searchExternalSource: vi.fn(),
	getExternalTrackPlayback: vi.fn(),
	hasExternalSource: false,
	externalSourceName: "SoundCloud" as string | undefined,
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
		getExternalTrackPlayback: mocks.getExternalTrackPlayback,
		getAlbumCoverUrl: (id: string) => `/api/v1/library/albums/${id}/cover`,
		getExternalTrackArtworkUrl: (id: string) =>
			`/api/v1/external/tracks/${id}/artwork`,
	},
}));
vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: (capability: string) =>
		capability === "external-source.v1" && mocks.hasExternalSource,
	useExternalSourceName: () =>
		mocks.hasExternalSource ? mocks.externalSourceName : undefined,
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

function renderDialog(
	onOpenChange = vi.fn(),
	{
		prefetchFirstExternalResult,
	}: { prefetchFirstExternalResult?: boolean } = {},
) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return {
		client,
		onOpenChange,
		...render(
			<QueryClientProvider client={client}>
				<LibrarySearchDialog
					open
					onOpenChange={onOpenChange}
					prefetchFirstExternalResult={prefetchFirstExternalResult}
				/>
			</QueryClientProvider>,
		),
	};
}
/** Keeps a test about hover or keyboard prefetch free of the automatic one. */
const withoutFirstPrefetch = { prefetchFirstExternalResult: false };

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
		mocks.externalSourceName = "SoundCloud";
	});

	it("shows no body before a query, only the input row and the footer", () => {
		renderDialog();
		const listbox = screen.getByRole("listbox", { name: "Search results" });
		expect(listbox.textContent).toBe("");
		expect(listbox.firstElementChild?.className).not.toMatch(/p-3/);
		expect(screen.queryByText(/Type to search/)).toBeNull();
	});

	describe("External Source", () => {
		const daftPunk: ExternalTrack = {
			id: "token.signature",
			title: "Around the World",
			artistName: "Daft Punk",
			durationMs: 429_000,
			webpageUrl: "https://soundcloud.com/daftpunk/around-the-world",
			extractor: "Soundcloud",
			hasArtwork: true,
		};

		beforeEach(() => {
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk],
			});
			mocks.playExternalTrack.mockResolvedValue(undefined);
			mocks.getExternalTrackPlayback.mockResolvedValue({
				format: "progressive",
				streamPath: "/api/v1/external/tracks/token.signature/stream",
			});
		});

		it("names the site in the search field only with the capability", () => {
			const { unmount } = renderDialog();
			const input = screen.getByRole("combobox", {
				name: "Search your library",
			});
			expect(input.getAttribute("placeholder")).toBe("Search your library");
			expect(
				screen.getByRole("dialog", { name: "Search your library" }),
			).toBeTruthy();
			unmount();

			mocks.hasExternalSource = true;
			renderDialog();
			const named = screen.getByRole("combobox", {
				name: "Search your library and SoundCloud",
			});
			expect(named.getAttribute("placeholder")).toBe(
				"Search your library and SoundCloud",
			);
			expect(
				screen.getByRole("dialog", {
					name: "Search your library and SoundCloud",
				}),
			).toBeTruthy();
		});

		it("stays hidden and never searches without the capability", async () => {
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByText("Nemo mix");
			expect(screen.queryByRole("group", { name: "SoundCloud" })).toBeNull();
			expect(mocks.searchExternalSource).not.toHaveBeenCalled();
		});

		it("lists External Tracks in their own section after the Library section", async () => {
			mocks.hasExternalSource = true;
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			const option = await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			expect(option.textContent).toContain("Daft Punk");
			expect(option.textContent).toContain("7:09");
			expect(option.textContent).not.toContain("Open");
			expect(mocks.searchExternalSource).toHaveBeenCalledWith(
				"ne",
				undefined,
				expect.any(AbortSignal),
			);
			const library = screen.getByRole("group", { name: "Library" });
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

		it("offers Install on External rows only when Soulseek Acquisition is offered", async () => {
			mocks.hasExternalSource = true;
			const { unmount } = renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			expect(screen.queryByRole("button", { name: /^Install/ })).toBeNull();
			unmount();

			const openInstall = vi.fn();
			const onOpenChange = vi.fn();
			const client = new QueryClient({
				defaultOptions: { queries: { retry: false } },
			});
			render(
				<QueryClientProvider client={client}>
					<SoulseekInstallContext.Provider
						value={{ canInstall: true, openInstall }}
					>
						<LibrarySearchDialog
							open
							onOpenChange={onOpenChange}
							prefetchFirstExternalResult={false}
						/>
					</SoulseekInstallContext.Provider>
				</QueryClientProvider>,
			);
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const install = await screen.findByRole("button", {
				name: "Install Around the World",
			});
			// Library Tracks are already installed.
			expect(screen.queryByRole("button", { name: "Install Nemo" })).toBeNull();
			fireEvent.click(install);

			expect(openInstall).toHaveBeenCalledWith(daftPunk);
			expect(onOpenChange).toHaveBeenCalledWith(false);
			expect(mocks.playExternalTrack).not.toHaveBeenCalled();
		});

		it("shows External artwork, with the globe when there is none or it fails", async () => {
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [
					daftPunk,
					{
						...daftPunk,
						id: "plain.signature",
						title: "No Cover",
						hasArtwork: false,
					},
				],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			const withCover = await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			const image = withCover.querySelector("img");
			expect(image?.getAttribute("src")).toBe(
				"/api/v1/external/tracks/token.signature/artwork",
			);
			expect(image?.getAttribute("class")).toContain("size-10");
			const withoutCover = within(external).getByRole("option", {
				name: /^No Cover/,
			});
			expect(withoutCover.querySelector("img")).toBeNull();
			expect(withoutCover.querySelector("svg.lucide-globe")).toBeTruthy();

			fireEvent.error(image as HTMLImageElement);
			expect(withCover.querySelector("img")).toBeNull();
			expect(withCover.querySelector("svg.lucide-globe")).toBeTruthy();
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

		it.each([
			[
				new ApiError(422, {
					error: "Unprocessable Entity",
					code: "external_track_unplayable",
					message: "no audio",
				}),
				"This track can't be played here",
			],
			[
				new ApiError(504, {
					error: "Gateway Timeout",
					code: "external_source_timeout",
					message: "slow",
				}),
				"The External Source took too long",
			],
			[
				new ApiError(502, {
					error: "Bad Gateway",
					code: "external_source_failed",
					message: "boom",
				}),
				"Couldn't play this track",
			],
			[new Error("network"), "Couldn't play this track"],
		])(
			"closes and reports a failed External play: %s",
			async (error, message) => {
				mocks.hasExternalSource = true;
				mocks.playExternalTrack.mockRejectedValue(error);
				const { onOpenChange } = renderDialog();
				fireEvent.change(screen.getByRole("combobox"), {
					target: { value: "ne" },
				});
				fireEvent.click(
					await screen.findByRole("option", { name: /^Around the World/ }),
				);
				expect(onOpenChange).toHaveBeenCalledWith(false);
				await waitFor(() =>
					expect(mocks.toastError).toHaveBeenCalledWith(message),
				);
			},
		);

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
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			expect(
				(await within(external).findByRole("alert")).textContent,
			).toContain("SoundCloud results could not be loaded.");
			expect(screen.getAllByRole("alert")).toHaveLength(1);
			const library = screen.getByRole("group", { name: "Library" });
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

		it("waits 500 ms before an External search while the library waits 200 ms", async () => {
			vi.useFakeTimers({ shouldAdvanceTime: false });
			mocks.hasExternalSource = true;
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await act(() => vi.advanceTimersByTimeAsync(200));
			expect(mocks.searchLibrary).toHaveBeenCalledWith("ne");
			expect(mocks.searchExternalSource).not.toHaveBeenCalled();
			const external = screen.getByRole("group", { name: "SoundCloud" });
			expect(within(external).getByText("Searching…")).toBeTruthy();
			await act(() => vi.advanceTimersByTimeAsync(299));
			expect(mocks.searchExternalSource).not.toHaveBeenCalled();
			await act(() => vi.advanceTimersByTimeAsync(1));
			expect(mocks.searchExternalSource).toHaveBeenCalledTimes(1);
			expect(mocks.searchExternalSource).toHaveBeenCalledWith(
				"ne",
				undefined,
				expect.any(AbortSignal),
			);
		});

		it("aborts a superseded External search", async () => {
			vi.useFakeTimers({ shouldAdvanceTime: false });
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockImplementation(
				() => new Promise(() => {}),
			);
			renderDialog();
			const input = screen.getByRole("combobox");
			fireEvent.change(input, { target: { value: "ne" } });
			await act(() => vi.advanceTimersByTimeAsync(500));
			const first = mocks.searchExternalSource.mock
				.calls[0]?.[2] as AbortSignal;
			expect(first.aborted).toBe(false);
			fireEvent.change(input, { target: { value: "nem" } });
			await act(() => vi.advanceTimersByTimeAsync(499));
			expect(mocks.searchExternalSource).toHaveBeenCalledTimes(1);
			await act(() => vi.advanceTimersByTimeAsync(1));
			expect(first.aborted).toBe(true);
			expect(mocks.searchExternalSource).toHaveBeenCalledTimes(2);
			expect(mocks.searchExternalSource.mock.calls[1]?.[0]).toBe("nem");
		});

		it("aborts a pending External search when the dialog closes", async () => {
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockImplementation(
				() => new Promise(() => {}),
			);
			const client = new QueryClient({
				defaultOptions: { queries: { retry: false } },
			});
			const view = (open: boolean) => (
				<QueryClientProvider client={client}>
					<LibrarySearchDialog open={open} onOpenChange={vi.fn()} />
				</QueryClientProvider>
			);
			const { rerender } = render(view(true));
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await waitFor(() =>
				expect(mocks.searchExternalSource).toHaveBeenCalled(),
			);
			const signal = mocks.searchExternalSource.mock
				.calls[0]?.[2] as AbortSignal;
			rerender(view(false));
			await waitFor(() => expect(signal.aborted).toBe(true));
		});

		it("prefetches playback for the External row reached with the keyboard", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo] });
			const playback = {
				format: "progressive",
				streamPath: "/api/v1/external/tracks/token.signature/stream",
			};
			mocks.getExternalTrackPlayback.mockResolvedValue(playback);
			const { client } = renderDialog(vi.fn(), withoutFirstPrefetch);
			const input = screen.getByRole("combobox");
			fireEvent.change(input, { target: { value: "ne" } });
			await screen.findByRole("option", { name: /^Around the World/ });
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(mocks.getExternalTrackPlayback).toHaveBeenCalledWith(
				daftPunk.id,
				expect.any(AbortSignal),
			);
			await waitFor(() =>
				expect(client.getQueryData(["external-playback", daftPunk.id])).toEqual(
					playback,
				),
			);
			// Back on the library row and down again: the cached resolve is reused.
			fireEvent.keyDown(input, { key: "ArrowUp" });
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(mocks.getExternalTrackPlayback).toHaveBeenCalledTimes(1);
		});

		it("prefetches playback after a 250 ms hover on an External row", async () => {
			vi.useFakeTimers({ shouldAdvanceTime: false });
			mocks.hasExternalSource = true;
			mocks.getExternalTrackPlayback.mockImplementation(
				() => new Promise(() => {}),
			);
			renderDialog(vi.fn(), withoutFirstPrefetch);
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await act(() => vi.advanceTimersByTimeAsync(500));
			// The search starts once the debounce commits; let it settle.
			await act(() => vi.advanceTimersByTimeAsync(10));
			const option = screen.getByRole("option", { name: /^Around the World/ });
			fireEvent.mouseEnter(option);
			await act(() => vi.advanceTimersByTimeAsync(249));
			fireEvent.mouseLeave(option);
			await act(() => vi.advanceTimersByTimeAsync(500));
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();

			fireEvent.mouseEnter(option);
			await act(() => vi.advanceTimersByTimeAsync(249));
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
			await act(() => vi.advanceTimersByTimeAsync(1));
			expect(mocks.getExternalTrackPlayback).toHaveBeenCalledWith(
				daftPunk.id,
				expect.any(AbortSignal),
			);
			// Library rows never prefetch.
			fireEvent.mouseEnter(screen.getByRole("option", { name: /Nemo mix/ }));
			await act(() => vi.advanceTimersByTimeAsync(500));
			expect(mocks.getExternalTrackPlayback).toHaveBeenCalledTimes(1);
		});

		it("keeps one playback prefetch in flight, the newest", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue(empty);
			const second = { ...daftPunk, id: "second.signature", title: "Da Funk" };
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk, second],
			});
			mocks.getExternalTrackPlayback.mockImplementation(
				() => new Promise(() => {}),
			);
			renderDialog(vi.fn(), withoutFirstPrefetch);
			const input = screen.getByRole("combobox");
			fireEvent.change(input, { target: { value: "ne" } });
			await screen.findByRole("option", { name: /^Da Funk/ });
			// The first row starts active; ArrowDown reaches the second, ArrowUp the first.
			fireEvent.keyDown(input, { key: "ArrowDown" });
			fireEvent.keyDown(input, { key: "ArrowUp" });
			expect(
				mocks.getExternalTrackPlayback.mock.calls.map(([id]) => id),
			).toEqual([second.id, daftPunk.id]);
			const [olderSignal, newerSignal] =
				mocks.getExternalTrackPlayback.mock.calls.map(
					([, signal]) => signal as AbortSignal,
				);
			await waitFor(() => expect(olderSignal?.aborted).toBe(true));
			expect(newerSignal?.aborted).toBe(false);
		});

		it("prefetches the first External result's playback once results arrive", async () => {
			mocks.hasExternalSource = true;
			const { client } = renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByRole("option", { name: /^Around the World/ });
			await waitFor(() =>
				expect(mocks.getExternalTrackPlayback).toHaveBeenCalledWith(
					daftPunk.id,
					expect.any(AbortSignal),
				),
			);
			await waitFor(() =>
				expect(
					client.getQueryData(["external-playback", daftPunk.id]),
				).toBeDefined(),
			);
			expect(mocks.getExternalTrackPlayback).toHaveBeenCalledTimes(1);
		});

		it("does not prefetch when there are no External results", async () => {
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByText("No results on SoundCloud");
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
		});

		it("does not prefetch a first result that is already resolved", async () => {
			mocks.hasExternalSource = true;
			const { client } = renderDialog();
			client.setQueryData(["external-playback", daftPunk.id], {
				format: "progressive",
				streamPath: "/cached",
			});
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByRole("option", { name: /^Around the World/ });
			await act(() => vi.advanceTimersByTimeAsync(50));
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
		});

		it("lets a hover on another External row replace the first-result prefetch", async () => {
			vi.useFakeTimers({ shouldAdvanceTime: false });
			mocks.hasExternalSource = true;
			const second = { ...daftPunk, id: "second.signature", title: "Da Funk" };
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk, second],
			});
			mocks.getExternalTrackPlayback.mockImplementation(
				() => new Promise(() => {}),
			);
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await act(() => vi.advanceTimersByTimeAsync(500));
			await act(() => vi.advanceTimersByTimeAsync(10));
			expect(
				mocks.getExternalTrackPlayback.mock.calls.map(([id]) => id),
			).toEqual([daftPunk.id]);
			fireEvent.mouseEnter(screen.getByRole("option", { name: /^Da Funk/ }));
			await act(() => vi.advanceTimersByTimeAsync(250));
			expect(
				mocks.getExternalTrackPlayback.mock.calls.map(([id]) => id),
			).toEqual([daftPunk.id, second.id]);
			const [firstSignal, hoverSignal] =
				mocks.getExternalTrackPlayback.mock.calls.map(
					([, signal]) => signal as AbortSignal,
				);
			expect(firstSignal?.aborted).toBe(true);
			expect(hoverSignal?.aborted).toBe(false);
		});

		it("does not prefetch the first result when the experiment is off", async () => {
			mocks.hasExternalSource = true;
			renderDialog(vi.fn(), withoutFirstPrefetch);
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByRole("option", { name: /^Around the World/ });
			await act(() => vi.advanceTimersByTimeAsync(50));
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
		});

		it("does not prefetch when the dialog closes before External results arrive", async () => {
			mocks.hasExternalSource = true;
			let finish!: (value: { source: string; items: ExternalTrack[] }) => void;
			mocks.searchExternalSource.mockImplementation(
				() =>
					new Promise((resolve) => {
						finish = resolve;
					}),
			);
			const client = new QueryClient({
				defaultOptions: { queries: { retry: false } },
			});
			const view = (open: boolean) => (
				<QueryClientProvider client={client}>
					<LibrarySearchDialog open={open} onOpenChange={vi.fn()} />
				</QueryClientProvider>
			);
			const { rerender } = render(view(true));
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await waitFor(() =>
				expect(mocks.searchExternalSource).toHaveBeenCalled(),
			);
			rerender(view(false));
			await act(async () => finish({ source: "scsearch", items: [daftPunk] }));
			await act(() => vi.advanceTimersByTimeAsync(50));
			expect(mocks.getExternalTrackPlayback).not.toHaveBeenCalled();
		});

		it("heads both sections alike, the External one with the site's name", async () => {
			mocks.hasExternalSource = true;
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const library = await screen.findByRole("group", { name: "Library" });
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			const libraryHeader = within(library).getByText("Library");
			const externalHeader = within(external).getByText(
				"External – SoundCloud",
			);
			expect(libraryHeader.className).toBe(externalHeader.className);
			expect(libraryHeader.querySelector("svg.lucide-library")).toBeTruthy();
			expect(externalHeader.querySelector("svg.lucide-globe")).toBeTruthy();
			// The kinds stay inside the Library section.
			expect(
				await within(library).findByRole("group", { name: "Playlist" }),
			).toBeTruthy();
			await within(external).findByRole("option", {
				name: /^Around the World/,
			});
		});

		it("falls back to External when the site has no name", async () => {
			mocks.hasExternalSource = true;
			mocks.externalSourceName = undefined;
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			const external = await screen.findByRole("group", { name: "External" });
			expect(within(external).getByText("External")).toBeTruthy();
			expect(
				await within(external).findByText("No External results"),
			).toBeTruthy();
		});

		it("says the library has no matches in its section while External results show", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue(empty);
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "re" },
			});
			const library = await screen.findByRole("group", { name: "Library" });
			const line = await within(library).findByText(
				"No matches in your library",
			);
			expect(line.tagName).toBe("P");
			expect(line.className).not.toMatch(/text-center|py-6/);
			const external = screen.getByRole("group", { name: "SoundCloud" });
			await within(external).findByRole("option", {
				name: /^Around the World/,
			});
			expect(within(external).queryByText(/No results/)).toBeNull();
			expect(
				library.compareDocumentPosition(external) &
					Node.DOCUMENT_POSITION_FOLLOWING,
			).toBeTruthy();
		});

		it("counts each source in the header", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue(empty);
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk, { ...daftPunk, id: "second", title: "Da Funk" }],
			});
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "re" },
			});
			await screen.findByRole("option", { name: /^Da Funk/ });
			const count = screen.getByRole("status");
			expect(count.tagName).toBe("OUTPUT");
			await waitFor(() =>
				expect(count.textContent).toBe("0 in library\u00a0· 2 on SoundCloud"),
			);
			// A narrow card keeps only the library count.
			expect(count.lastElementChild?.className).toContain("hidden");
			expect(count.lastElementChild?.className).toContain("@lg:inline");
		});

		it("counts unnamed External results without a site", async () => {
			mocks.hasExternalSource = true;
			mocks.externalSourceName = undefined;
			mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo] });
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByRole("option", { name: /^Around the World/ });
			await waitFor(() =>
				expect(screen.getByRole("status").textContent).toBe(
					"1 in library\u00a0· 1 external",
				),
			);
		});

		it("counts the library alone while External results load", async () => {
			mocks.hasExternalSource = true;
			mocks.searchExternalSource.mockImplementation(
				() => new Promise(() => {}),
			);
			mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo] });
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "ne" },
			});
			await screen.findByText("Nemo");
			expect(screen.getByRole("status").textContent).toBe("1 in library");
		});

		it("moves across both sections with the arrow keys, wrapping at the ends", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue({ ...empty, tracks: [nemo] });
			const second = { ...daftPunk, id: "second.signature", title: "Da Funk" };
			mocks.searchExternalSource.mockResolvedValue({
				source: "scsearch",
				items: [daftPunk, second],
			});
			renderDialog(vi.fn(), withoutFirstPrefetch);
			const input = screen.getByRole("combobox");
			fireEvent.change(input, { target: { value: "ne" } });
			await screen.findByRole("option", { name: /^Da Funk/ });
			const library = screen.getByRole("group", { name: "Library" });
			const external = screen.getByRole("group", { name: "SoundCloud" });
			const active = () => {
				const id = input.getAttribute("aria-activedescendant");
				const option = id ? document.getElementById(id) : null;
				expect(option?.getAttribute("aria-selected")).toBe("true");
				expect(screen.getAllByRole("option", { selected: true })).toEqual([
					option,
				]);
				return option;
			};
			expect(library.contains(active())).toBe(true);
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(
				within(external).getByRole("option", { name: /^Around the World/ }),
			).toBe(active());
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(within(external).getByRole("option", { name: /^Da Funk/ })).toBe(
				active(),
			);
			fireEvent.keyDown(input, { key: "ArrowDown" });
			expect(library.contains(active())).toBe(true);
			fireEvent.keyDown(input, { key: "ArrowUp" });
			expect(within(external).getByRole("option", { name: /^Da Funk/ })).toBe(
				active(),
			);
			expect(document.activeElement).toBe(input);
		});

		it("shows an External failure inside its section", async () => {
			mocks.hasExternalSource = true;
			mocks.searchLibrary.mockResolvedValue(empty);
			mocks.searchExternalSource.mockRejectedValue(new Error("boom"));
			renderDialog();
			fireEvent.change(screen.getByRole("combobox"), {
				target: { value: "re" },
			});
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			const alert = await within(external).findByRole("alert");
			expect(alert.textContent).toContain(
				"SoundCloud results could not be loaded.",
			);
			expect(within(alert).getByRole("button", { name: "Retry" })).toBeTruthy();
			const library = screen.getByRole("group", { name: "Library" });
			expect(
				await within(library).findByText("No matches in your library"),
			).toBeTruthy();
			expect(screen.getByRole("status").textContent).toBe("0 in library");
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
			const external = await screen.findByRole("group", { name: "SoundCloud" });
			await screen.findByText("Nemo mix");
			expect(within(external).getByText("Searching…")).toBeTruthy();
			await waitFor(() =>
				expect(mocks.searchExternalSource).toHaveBeenCalled(),
			);
			expect(within(external).getByText("Searching…")).toBeTruthy();
			await act(async () => finish({ source: "scsearch", items: [] }));
			expect(
				await within(external).findByText("No results on SoundCloud"),
			).toBeTruthy();
		});
	});

	it("shows Best Match before ordered nonempty categories", async () => {
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "ne" },
		});

		const section = await screen.findByRole("group", { name: "Library" });
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

	it("tells the user when nothing matches, in the Library section", async () => {
		mocks.searchLibrary.mockResolvedValue(empty);
		renderDialog();
		fireEvent.change(screen.getByRole("combobox"), {
			target: { value: "zzz" },
		});
		const library = await screen.findByRole("group", { name: "Library" });
		expect(within(library).getByText("Library")).toBeTruthy();
		expect(
			await within(library).findByText("No matches in your library"),
		).toBeTruthy();
		expect(screen.getAllByRole("group")).toEqual([library]);
		expect(screen.getByRole("status").textContent).toBe("0 results");
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
		expect(screen.queryByText(/No matches in your library/)).toBeNull();
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
		expect(await screen.findByText("No matches in your library")).toBeTruthy();
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
		expect(screen.queryByText(/No matches in your library/)).toBeNull();
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
