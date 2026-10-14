import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
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
import { PlaylistDetailContent } from "./-playlist-page";
import { PlaylistsPage } from "./-playlists-page";

const mocks = vi.hoisted(() => ({
	listPlaylists: vi.fn(),
	getPlaylist: vi.fn(),
	addPlaylistTrack: vi.fn(),
	removePlaylistTrack: vi.fn(),
	playTrack: vi.fn(),
	queueTracks: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("@tanstack/react-router", async () => {
	const actual = await vi.importActual<typeof import("@tanstack/react-router")>(
		"@tanstack/react-router",
	);
	return {
		...actual,
		Link: ({
			children,
			to,
			params,
			...props
		}: {
			children: React.ReactNode;
			to: string;
			params?: Record<string, string>;
		}) => (
			<a
				href={params ? to.replace("$playlistId", params.playlistId) : to}
				{...props}
			>
				{children}
			</a>
		),
	};
});

vi.mock("#/lib/api", () => ({
	apiClient: {
		listPlaylists: mocks.listPlaylists,
		getPlaylist: mocks.getPlaylist,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		getAlbum: vi.fn(async (albumId: string) => ({
			id: albumId,
			title: `Album ${albumId}`,
			artistName: "New Order",
			artistId: "artist-1",
		})),
		addPlaylistTrack: mocks.addPlaylistTrack,
		removePlaylistTrack: mocks.removePlaylistTrack,
	},
}));

vi.mock("@repo/ui", () => ({
	toast: { error: mocks.toastError },
	AlbumArt: ({
		coverUrl,
		className,
	}: {
		coverUrl?: string | null;
		className?: string;
	}) => (coverUrl ? <img alt="" src={coverUrl} className={className} /> : null),
	usePlayback: () => ({
		playTrack: mocks.playTrack,
		queueTracks: mocks.queueTracks,
		currentTrack: null,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	}),
}));

const tracks = [
	{
		id: "t1",
		title: "Blue Monday",
		artistName: "New Order",
		albumId: "a1",
		trackNo: 1,
		durationMs: 180_000,
		format: "flac",
		genre: "Synthpop",
	},
	{
		id: "t2",
		title: "Bizarre Love Triangle",
		artistName: "New Order",
		albumId: "a2",
		trackNo: 1,
		durationMs: 240_000,
		format: "flac",
		genre: "Synthpop",
	},
	{
		id: "t3",
		title: "Age of Consent",
		artistName: "New Order",
		albumId: "a3",
		trackNo: 1,
		durationMs: 315_000,
		format: "flac",
		genre: "Rock",
	},
	{
		id: "t4",
		title: "Temptation",
		artistName: "New Order",
		albumId: "a4",
		trackNo: 1,
		durationMs: 300_000,
		format: "flac",
		genre: "Dance",
	},
];

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("playlist routes", () => {
	it("reports a failed bulk queue and stops before adding later album runs", async () => {
		const error = new Error("Queue unavailable");
		mocks.queueTracks.mockRejectedValueOnce(error);
		const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
		try {
			renderWithQuery(<PlaylistDetailContent playlistId="p1" />);
			await screen.findByRole("heading", { name: "Favorites" });
			fireEvent.click(screen.getByRole("button", { name: "Queue" }));
			await waitFor(() =>
				expect(mocks.toastError).toHaveBeenCalledWith(
					"Failed to queue collection tracks",
				),
			);
			expect(warning).toHaveBeenCalledWith(
				"Failed to queue collection tracks",
				{
					trackIds: ["t1", "t2", "t3", "t4"],
					error,
				},
			);
			expect(mocks.queueTracks).toHaveBeenCalledTimes(1);
		} finally {
			warning.mockRestore();
		}
	});

	beforeEach(() => {
		mocks.listPlaylists.mockClear();
		mocks.getPlaylist.mockClear();
		mocks.listPlaylists.mockResolvedValue({
			items: [{ id: "p1", name: "Favorites", isDefault: true, trackCount: 4 }],
		});
		mocks.getPlaylist.mockResolvedValue({
			id: "p1",
			name: "Favorites",
			isDefault: true,
			trackCount: 4,
			tracks,
		});
		mocks.removePlaylistTrack.mockResolvedValue({
			id: "p1",
			name: "Favorites",
			isDefault: true,
			trackCount: 0,
			tracks: [],
		});
		mocks.addPlaylistTrack.mockResolvedValue({
			id: "p1",
			name: "Favorites",
			isDefault: true,
			trackCount: 1,
			tracks: [tracks[0]],
		});
		mocks.playTrack.mockClear();
		mocks.queueTracks.mockReset().mockResolvedValue(undefined);
		mocks.toastError.mockClear();
		mocks.removePlaylistTrack.mockClear();
		mocks.addPlaylistTrack.mockClear();
	});

	afterEach(() => {
		cleanup();
	});

	it("links playlist cards to detail pages without card playback actions", async () => {
		const { container } = renderWithQuery(<PlaylistsPage />);

		const header = await screen.findByRole("heading", { name: "Playlists" });
		expect(header.closest("header")?.className).toContain("sticky");
		expect(screen.getByTestId("playlists-page-shell").className).toContain(
			"overflow-hidden",
		);
		expect(screen.getByTestId("playlists-page-content").className).toContain(
			"[scrollbar-width:none]",
		);
		expect(
			header.closest("header")?.querySelector(".page-content-column"),
		).toBeTruthy();
		expect(
			screen
				.getByTestId("playlists-page-content")
				.querySelector(".page-content-column"),
		).toBeTruthy();
		expect(
			screen
				.getByTestId("playlists-page-content")
				.querySelector('[class*="auto-fill"]'),
		).toBeTruthy();

		const link = await screen.findByRole("link", {
			name: /Favorites 4 tracks/,
		});

		expect(link.getAttribute("href")).toBe("/playlists/p1");
		expect(link.className).toContain("aspect-square");
		expect(link.className).toContain("overflow-hidden");
		expect(link.className).toContain("rounded-md");
		expect(link.className).toContain("hover:-translate-y-px");
		expect(link.className).toContain("focus-visible:ring-2");
		expect(link.className).toContain("motion-reduce:transition-none");
		expect(link.className).not.toContain("hover:-translate-y-1");
		expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
		expect(screen.queryByRole("button", { name: "Shuffle" })).toBeNull();
		await waitFor(() => {
			expect(container.querySelectorAll("img")).toHaveLength(4);
		});
		expect(
			Array.from(container.querySelectorAll("img")).map((img) =>
				img.getAttribute("src"),
			),
		).toEqual(
			expect.arrayContaining([
				"/cover/a1",
				"/cover/a2",
				"/cover/a3",
				"/cover/a4",
			]),
		);
		const overlay = container.querySelector("[data-playlist-card-overlay]");
		expect(overlay?.className).toContain("px-3");
		expect(overlay?.className).toContain("pt-14");
		expect(overlay?.querySelector("p")?.className).toContain("font-semibold");
		expect(overlay?.querySelector("p")?.className).not.toContain(
			"group-hover:underline",
		);
		expect(
			container.querySelector('[data-testid="playlist-card-cover-stack"]'),
		).toBeTruthy();
	});

	it("keeps the playlist header visible while showing the shared loading grid", () => {
		mocks.listPlaylists.mockReturnValue(new Promise(() => undefined));

		renderWithQuery(<PlaylistsPage />);

		expect(screen.getByRole("heading", { name: "Playlists" })).toBeTruthy();
		expect(screen.getByRole("status").textContent).toContain(
			"Loading playlists",
		);
		expect(screen.getAllByTestId("collection-card-skeleton")).toHaveLength(10);
	});

	it("retries the playlist query from the shared error panel", async () => {
		mocks.listPlaylists.mockRejectedValue(new Error("playlist request failed"));

		renderWithQuery(<PlaylistsPage />);

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain("Unable to load playlists");
		expect(alert.textContent).toContain("Check your connection and try again.");
		fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
		await waitFor(() => {
			expect(mocks.listPlaylists.mock.calls.length).toBeGreaterThanOrEqual(2);
		});
	});

	it("renders the shared playlist empty state without an action", async () => {
		mocks.listPlaylists.mockResolvedValue({ items: [] });

		renderWithQuery(<PlaylistsPage />);

		expect(await screen.findByText("No playlists yet")).toBeTruthy();
		expect(
			screen.getByText("Create a playlist to organize your library."),
		).toBeTruthy();
		expect(screen.queryByRole("button")).toBeNull();
	});

	it("plays queues and removes tracks from playlist detail", async () => {
		const { container } = renderWithQuery(
			<PlaylistDetailContent playlistId="p1" />,
		);

		await screen.findByRole("heading", { name: "Favorites" });
		expect(
			screen.queryByRole("link", { name: /Back to playlists/ }),
		).toBeNull();
		expect(screen.getByTestId("playlist-detail-content").className).toContain(
			"page-content-column",
		);
		const searchInput = screen.getByPlaceholderText("Search Favorites…");
		expect(
			searchInput.closest('[data-testid="playlist-track-search"]'),
		).toBeTruthy();
		expect(searchInput.closest("header")).toBeNull();
		expect(
			container.querySelector('[data-testid="playlist-track-search"]')
				?.className,
		).toContain("w-full");

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenCalledWith(
			"t1",
			["t1", "t2", "t3", "t4"],
			{
				kind: "playlist",
				playlistId: "p1",
				name: "Favorites",
			},
		);

		fireEvent.click(screen.getByRole("button", { name: "Shuffle" }));
		expect(mocks.playTrack).toHaveBeenLastCalledWith(
			expect.any(String),
			expect.arrayContaining(["t1", "t2", "t3", "t4"]),
			{ kind: "playlist", playlistId: "p1", name: "Favorites" },
		);
		fireEvent.doubleClick(screen.getByText("Blue Monday"));
		expect(mocks.playTrack).toHaveBeenLastCalledWith(
			"t1",
			["t1", "t2", "t3", "t4"],
			{
				kind: "playlist",
				playlistId: "p1",
				name: "Favorites",
			},
		);

		fireEvent.click(screen.getByRole("button", { name: "Queue" }));
		await waitFor(() => expect(mocks.queueTracks).toHaveBeenCalledTimes(4));
		tracks.forEach((track, index) => {
			expect(mocks.queueTracks).toHaveBeenNthCalledWith(index + 1, [track.id], {
				kind: "album",
				albumId: track.albumId,
				albumTitle: `Album ${track.albumId}`,
				artistName: "New Order",
			});
		});

		await waitFor(() => {
			expect(
				screen.getAllByRole("button", { name: "Remove from favorites" }),
			).toHaveLength(4);
		});
		expect(
			screen.getByRole("row", { name: /Blue Monday/ }).querySelector("td")
				?.className,
		).toContain("py-1.5");
		expect(screen.getByRole("row", { name: /1 Blue Monday/ })).toBeTruthy();
		expect(
			screen.getByRole("row", { name: /2 Bizarre Love Triangle/ }),
		).toBeTruthy();
		expect(screen.getByRole("row", { name: /3 Age of Consent/ })).toBeTruthy();

		fireEvent.contextMenu(screen.getByRole("row", { name: /Blue Monday/ }));
		expect(screen.getByText("Details")).toBeTruthy();
		expect(screen.queryByText("Delete track")).toBeNull();
		fireEvent.click(screen.getByText("Remove from playlist"));

		await waitFor(() => {
			expect(mocks.removePlaylistTrack).toHaveBeenCalledWith("p1", "t1");
		});
	});

	it("filters playlist detail actions to the visible tracks", async () => {
		const { container } = renderWithQuery(
			<PlaylistDetailContent playlistId="p1" />,
		);

		await screen.findByRole("heading", { name: "Favorites" });
		expect(
			container.querySelector('[data-testid="collection-cover-stack"]'),
		).toBeTruthy();
		expect(
			container
				.querySelector('[data-testid="collection-cover-stack"]')
				?.querySelectorAll("img"),
		).toHaveLength(4);
		expect(
			new Set(
				Array.from(
					container
						.querySelector('[data-testid="collection-cover-stack"]')
						?.querySelectorAll("img") ?? [],
				).map((img) => img.getAttribute("src")),
			),
		).toEqual(new Set(["/cover/a1", "/cover/a2", "/cover/a3", "/cover/a4"]));

		fireEvent.change(screen.getByPlaceholderText("Search Favorites…"), {
			target: { value: "temptation" },
		});

		expect(screen.queryByText("Blue Monday")).toBeNull();
		expect(screen.getByText("Temptation")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenCalledWith("t4", ["t4"], {
			kind: "playlist",
			playlistId: "p1",
			name: "Favorites",
		});

		fireEvent.click(screen.getByRole("button", { name: "Queue" }));
		await waitFor(() =>
			expect(mocks.queueTracks).toHaveBeenCalledExactlyOnceWith(["t4"], {
				kind: "album",
				albumId: "a4",
				albumTitle: "Album a4",
				artistName: "New Order",
			}),
		);
	});
});
