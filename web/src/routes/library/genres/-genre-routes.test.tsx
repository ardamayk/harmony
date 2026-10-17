import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { GenreDetailContent } from "./-genre-page";
import { GenresPage } from "./-genres-page";

const mocks = vi.hoisted(() => ({
	listTracks: vi.fn(),
	listPlaylists: vi.fn(),
	getPlaylist: vi.fn(),
	addPlaylistTrack: vi.fn(),
	removePlaylistTrack: vi.fn(),
	playTrack: vi.fn(),
	queueTracks: vi.fn(),
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
			<a href={params ? to.replace("$genre", params.genre) : to} {...props}>
				{children}
			</a>
		),
	};
});

vi.mock("#/lib/api", () => ({
	apiClient: {
		listTracks: mocks.listTracks,
		listPlaylists: mocks.listPlaylists,
		getPlaylist: mocks.getPlaylist,
		addPlaylistTrack: mocks.addPlaylistTrack,
		removePlaylistTrack: mocks.removePlaylistTrack,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		getAlbum: vi.fn(async (albumId: string) => ({
			id: albumId,
			title: `Album ${albumId}`,
			artistName: "New Order",
			artistId: "artist-1",
		})),
	},
}));

vi.mock("@repo/ui", () => ({
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
		durationMs: 180_000,
		format: "flac",
		genre: "Synthpop",
	},
	{
		id: "t2",
		title: "Bizarre Love Triangle",
		artistName: "New Order",
		albumId: "a2",
		durationMs: 240_000,
		format: "flac",
		genre: "Synthpop; Dance",
	},
	{
		id: "t3",
		title: "Age of Consent",
		artistName: "New Order",
		albumId: "a3",
		durationMs: 315_000,
		format: "flac",
		genre: "Synthpop",
	},
	{
		id: "t4",
		title: "Temptation",
		artistName: "New Order",
		albumId: "a4",
		durationMs: 300_000,
		format: "flac",
		genre: "Synthpop",
	},
];

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("genre routes", () => {
	beforeEach(() => {
		mocks.listTracks.mockResolvedValue({ items: tracks });
		mocks.listPlaylists.mockResolvedValue({
			items: [{ id: "favorites", name: "Favorites", isDefault: true }],
		});
		mocks.getPlaylist.mockResolvedValue({ tracks: [] });
		mocks.addPlaylistTrack.mockResolvedValue({ tracks: [] });
		mocks.removePlaylistTrack.mockResolvedValue({ tracks: [] });
		mocks.playTrack.mockClear();
		mocks.queueTracks.mockClear();
	});

	afterEach(() => {
		cleanup();
	});

	it("renders genre cards derived from track metadata", async () => {
		const { container } = renderWithQuery(<GenresPage />);

		const synthpop = await screen.findByRole("link", {
			name: /Synthpop 4 tracks/,
		});
		const dance = screen.getByRole("link", { name: /Dance 1 track/ });

		expect(synthpop.getAttribute("href")).toBe("/library/genres/Synthpop");
		expect(dance.getAttribute("href")).toBe("/library/genres/Dance");
		expect(synthpop.className).toContain("duration-300");
		expect(synthpop.className).toContain("ease-out");
		expect(synthpop.className).toContain("hover:-translate-y-1");
		expect(synthpop.className).toContain(
			"hover:border-shell-active-foreground/35",
		);
		expect(synthpop.className).toContain("focus-visible:-translate-y-1");
		expect(synthpop.className).toContain("motion-reduce:transition-none");
		expect(synthpop.className).toContain("aspect-square");
		expect(synthpop.className).toContain("overflow-hidden");
		expect(screen.getByTestId("genres-page-shell").className).toContain(
			"overflow-hidden",
		);
		expect(screen.getByTestId("genres-page-content").className).toContain(
			"scrollbar-hidden",
		);
		expect(
			screen
				.getByTestId("genres-page-content")
				.querySelector(".page-content-column"),
		).toBeTruthy();
		expect(
			screen
				.getByTestId("genres-page-content")
				.querySelector(".grid-cols-cards"),
		).toBeTruthy();
		expect(
			container.querySelector('[data-testid="playlist-card-cover-stack"]'),
		).toBeTruthy();
		const overlay = container.querySelector("[data-genre-card-overlay]");
		expect(overlay?.className).toContain("px-3");
		expect(overlay?.className).toContain("pt-12");
		expect(overlay?.querySelector("p")?.className).toContain("font-semibold");
		expect(overlay?.querySelector("p")?.className).not.toContain(
			"group-hover:underline",
		);
		await waitFor(() => {
			expect(container.querySelectorAll("img").length).toBeGreaterThanOrEqual(
				4,
			);
		});
	});

	it("renders structured Genre names without punctuation guessing", async () => {
		mocks.listTracks.mockResolvedValue({
			items: [
				{
					...tracks[0],
					genre: "Legacy, Guess",
					genres: [{ id: "ambient", name: "Electronic / Ambient" }],
				},
			],
		});

		renderWithQuery(<GenresPage />);

		expect(
			await screen.findByRole("link", {
				name: /Electronic \/ Ambient 1 track/,
			}),
		).toBeTruthy();
		expect(screen.queryByRole("link", { name: /Legacy/ })).toBeNull();
		expect(screen.queryByRole("link", { name: /Guess/ })).toBeNull();
	});

	it("scopes detail playback actions to matching genre tracks", async () => {
		renderWithQuery(<GenreDetailContent genre="Synthpop" />);

		await screen.findByRole("heading", { name: "Synthpop" });
		expect(screen.queryByRole("link", { name: /Back to genres/ })).toBeNull();
		expect(screen.getByTestId("genre-detail-content").className).toContain(
			"page-content-column",
		);

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenCalledWith("t1", [
			"t1",
			"t2",
			"t3",
			"t4",
		]);

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

		expect(screen.getByText("Blue Monday")).toBeTruthy();
		expect(screen.getByText("Bizarre Love Triangle")).toBeTruthy();
		expect(
			document.querySelector('[data-testid="collection-cover-stack"]'),
		).toBeTruthy();
		expect(
			screen.getAllByRole("button", { name: "Add to favorites" }),
		).toHaveLength(4);
		expect(
			screen.getByRole("row", { name: /Blue Monday/ }).querySelector("td")
				?.className,
		).toContain("py-2");
	});

	it("lists genre tracks in added order with list numbering", async () => {
		mocks.listTracks.mockResolvedValue({
			items: [
				{ ...tracks[0], trackNo: 9, createdAt: "2026-03-01T00:00:00Z" },
				{ ...tracks[1], trackNo: 4, createdAt: "2026-01-01T00:00:00Z" },
				{ ...tracks[2], trackNo: 7, createdAt: "2026-02-01T00:00:00Z" },
			],
		});
		renderWithQuery(<GenreDetailContent genre="Synthpop" />);

		await screen.findByRole("heading", { name: "Synthpop" });
		const rows = screen
			.getAllByRole("row")
			.slice(1)
			.map((row) => row.textContent ?? "");
		expect(rows[0]).toMatch(/^1.*Bizarre Love Triangle/);
		expect(rows[1]).toMatch(/^2.*Age of Consent/);
		expect(rows[2]).toMatch(/^3.*Blue Monday/);

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenCalledWith("t2", ["t2", "t3", "t1"]);
	});

	it("filters genre detail actions to the visible tracks", async () => {
		const { container } = renderWithQuery(
			<GenreDetailContent genre="Synthpop" />,
		);

		await screen.findByRole("heading", { name: "Synthpop" });
		expect(screen.getByTestId("genre-track-search")).toBeTruthy();
		expect(screen.getByPlaceholderText("Search Synthpop…").className).toContain(
			"h-11",
		);
		expect(
			container
				.querySelector('[data-testid="collection-cover-stack"]')
				?.querySelectorAll("img"),
		).toHaveLength(4);
		expect(container.querySelectorAll("img")).toHaveLength(8);

		fireEvent.change(screen.getByPlaceholderText("Search Synthpop…"), {
			target: { value: "bizarre" },
		});

		expect(screen.queryByText("Blue Monday")).toBeNull();
		expect(screen.getByText("Bizarre Love Triangle")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenCalledWith("t2", ["t2"]);

		fireEvent.click(screen.getByRole("button", { name: "Queue" }));
		await waitFor(() =>
			expect(mocks.queueTracks).toHaveBeenCalledExactlyOnceWith(["t2"], {
				kind: "album",
				albumId: "a2",
				albumTitle: "Album a2",
				artistName: "New Order",
			}),
		);
	});
});
