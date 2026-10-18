import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { ArtistDetailContent } from "./-artist-page";

const mocks = vi.hoisted(() => ({
	getArtist: vi.fn(),
	listAlbums: vi.fn(),
	getArtistBiography: vi.fn(),
	listArtistPopularTracks: vi.fn(),
}));

// The Album grid has its own tests; here it only lists what it was given.
vi.mock("#/components/album-grid", () => ({
	AlbumGrid: ({ albums }: { albums: Array<{ id: string; title: string }> }) => (
		<ul>
			{albums.map((album) => (
				<li key={album.id}>{album.title}</li>
			))}
		</ul>
	),
}));

// The Track list has its own tests; here it only lists what it was given.
vi.mock("#/components/track-list", () => ({
	TrackList: ({ tracks }: { tracks: Array<{ id: string; title: string }> }) => (
		<ol aria-label="Popular tracks">
			{tracks.map((track) => (
				<li key={track.id}>{track.title}</li>
			))}
		</ol>
	),
}));

vi.mock("@repo/ui", async (importOriginal) => ({
	...(await importOriginal<typeof import("@repo/ui")>()),
	useCoverAccent: () => ({}),
	useLayout: () => ({ preferences: { theme: { mode: "dark" } } }),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		getArtist: mocks.getArtist,
		listAlbums: mocks.listAlbums,
		getArtistBiography: mocks.getArtistBiography,
		listArtistPopularTracks: mocks.listArtistPopularTracks,
		getArtistPhotoUrl: (artistId: string) => `/photo/${artistId}`,
	},
}));

function renderPage() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<ArtistDetailContent artistId="artist-1" />
		</QueryClientProvider>,
	);
}

const album = {
	id: "album-1",
	title: "Awake",
	artistId: "artist-1",
	artistName: "Skillet",
	albumArtists: [{ id: "artist-1", name: "Skillet" }],
	releaseIdentifiers: [],
	genreItems: [],
	year: 2009,
};

describe("artist detail route", () => {
	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});
	beforeEach(() => {
		mocks.listArtistPopularTracks.mockResolvedValue([]);
	});

	it("shows the photo, the albums and the Wikipedia summary with its source", async () => {
		mocks.getArtist.mockResolvedValue({
			id: "artist-1",
			name: "Skillet",
			albumCount: 1,
		});
		mocks.listAlbums.mockResolvedValue({ items: [album], total: 1 });
		mocks.getArtistBiography.mockResolvedValue({
			text: "Skillet is an American rock band.",
			sourceUrl: "https://en.wikipedia.org/wiki/Skillet_(band)",
		});

		renderPage();

		expect(
			await screen.findByRole("heading", { name: "Skillet" }),
		).toBeTruthy();
		expect(screen.getByText("1 album")).toBeTruthy();
		expect(mocks.listAlbums).toHaveBeenCalledWith({
			artistId: "artist-1",
			limit: 100,
		});
		expect(await screen.findByText("Awake")).toBeTruthy();
		expect(
			await screen.findByText("Skillet is an American rock band."),
		).toBeTruthy();
		const source = screen.getByRole("link", {
			name: "From Wikipedia, CC BY-SA",
		});
		expect(source.getAttribute("href")).toBe(
			"https://en.wikipedia.org/wiki/Skillet_(band)",
		);
		const photo = document.querySelector('img[src="/photo/artist-1"]');
		expect(photo?.className).toContain("rounded-full");
	});

	it("leaves the About section out when there is no biography", async () => {
		mocks.getArtist.mockResolvedValue({ id: "artist-1", name: "Solo" });
		mocks.listAlbums.mockResolvedValue({ items: [], total: 0 });
		mocks.getArtistBiography.mockResolvedValue(null);

		renderPage();

		expect(await screen.findByRole("heading", { name: "Solo" })).toBeTruthy();
		expect(screen.getByText("0 albums")).toBeTruthy();
		expect(
			await screen.findByText("No albums under this artist's name."),
		).toBeTruthy();
		expect(screen.queryByText("From Wikipedia, CC BY-SA")).toBeNull();
	});

	it("filters the discography and clips a long biography until Read more", async () => {
		mocks.getArtist.mockResolvedValue({
			id: "artist-1",
			name: "Skillet",
			albumCount: 2,
		});
		mocks.listAlbums.mockResolvedValue({
			items: [
				{ ...album, trackCount: 12 },
				{ ...album, id: "album-2", title: "Hero", trackCount: 1 },
			],
			total: 2,
		});
		mocks.getArtistBiography.mockResolvedValue({
			text: "Skillet is an American rock band. ".repeat(10),
			sourceUrl: "https://en.wikipedia.org/wiki/Skillet_(band)",
		});

		renderPage();

		const readMore = await screen.findByRole("button", { name: "Read more" });
		expect(readMore.getAttribute("aria-expanded")).toBe("false");
		fireEvent.click(readMore);
		expect(
			screen
				.getByRole("button", { name: "Show less" })
				.getAttribute("aria-expanded"),
		).toBe("true");

		expect(await screen.findByText("Hero")).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "Albums" }));
		expect(screen.queryByText("Hero")).toBeNull();
		expect(screen.getByText("Awake")).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "Singles and EPs" }));
		expect(screen.getByText("Hero")).toBeTruthy();
		expect(screen.queryByText("Awake")).toBeNull();
	});

	it("lists five popular tracks, then up to ten after See more", async () => {
		mocks.getArtist.mockResolvedValue({ id: "artist-1", name: "Skillet" });
		mocks.listAlbums.mockResolvedValue({ items: [], total: 0 });
		mocks.getArtistBiography.mockResolvedValue(null);
		mocks.listArtistPopularTracks.mockResolvedValue(
			Array.from({ length: 7 }, (_, index) => ({
				id: `t${index}`,
				title: `Song ${index + 1}`,
			})),
		);

		renderPage();

		const list = await screen.findByRole("list", { name: "Popular tracks" });
		expect(mocks.listArtistPopularTracks).toHaveBeenCalledWith("artist-1", 10);
		expect(list.children).toHaveLength(5);
		fireEvent.click(screen.getByRole("button", { name: "See more" }));
		expect(
			screen.getByRole("list", { name: "Popular tracks" }).children,
		).toHaveLength(7);
		expect(screen.getByRole("button", { name: "Show less" })).toBeTruthy();
	});

	it("leaves Popular out when no library track is among them", async () => {
		mocks.getArtist.mockResolvedValue({ id: "artist-1", name: "Solo" });
		mocks.listAlbums.mockResolvedValue({ items: [], total: 0 });
		mocks.getArtistBiography.mockResolvedValue(null);

		renderPage();

		expect(await screen.findByRole("heading", { name: "Solo" })).toBeTruthy();
		expect(screen.queryByRole("heading", { name: "Popular" })).toBeNull();
	});
});
