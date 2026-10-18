import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { ArtistDetailContent } from "./-artist-page";

const mocks = vi.hoisted(() => ({
	getArtist: vi.fn(),
	listAlbums: vi.fn(),
	getArtistBiography: vi.fn(),
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
		expect(screen.queryByRole("heading", { name: "About" })).toBeNull();
	});
});
