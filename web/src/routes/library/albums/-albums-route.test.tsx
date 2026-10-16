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
import { AlbumsPage } from "./-albums-page";

const mocks = vi.hoisted(() => ({
	listAlbums: vi.fn(),
	listArtists: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		listAlbums: mocks.listAlbums,
		listArtists: mocks.listArtists,
	},
}));

vi.mock("#/components/album-grid", () => ({
	AlbumGrid: ({ albums }: { albums: Array<{ id: string; title: string }> }) => (
		<div>
			{albums.map((album) => (
				<p key={album.id}>{album.title}</p>
			))}
		</div>
	),
}));

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("albums route", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		mocks.listArtists.mockResolvedValue({
			items: [{ id: "artist-1", name: "Taylor Swift" }],
		});
		const albumItems = [
			{
				id: "album-1",
				title: "1989",
				artistName: "Taylor Swift",
				genres: ["Pop"],
				trackCount: 2,
			},
		];
		mocks.listAlbums.mockImplementation(({ q }: { q?: string }) =>
			Promise.resolve({ items: q ? [] : albumItems }),
		);
	});

	afterEach(() => {
		cleanup();
	});

	it("keeps the page title for screen readers only and hides the page scrollbar", async () => {
		renderWithQuery(<AlbumsPage />);

		await screen.findByText("1989");
		const heading = screen.getByRole("heading", { name: "Albums" });

		// The lit Top Nav section already names the page.
		expect(heading.className).toContain("sr-only");
		expect(heading.closest("header")?.className).toContain("sr-only");
		expect(screen.getByTestId("albums-page-shell").className).toContain(
			"overflow-hidden",
		);
		expect(screen.getByTestId("albums-page-content").className).toContain(
			"[scrollbar-width:none]",
		);
		expect(screen.getByTestId("albums-page-content").className).toContain(
			"py-6",
		);
		expect(screen.getByTestId("albums-page-content").className).not.toContain(
			"pt-8",
		);
		const collectionContainer = screen
			.getByTestId("albums-page-content")
			.querySelector(".page-content-column");
		expect(collectionContainer).toBeTruthy();
		expect(screen.getByRole("button", { name: "Filters" })).toBeTruthy();
		expect(screen.queryByRole("button", { name: "Scan library" })).toBeNull();
	});

	it("opens album filters in a drawer from the Top Nav filter button", async () => {
		renderWithQuery(<AlbumsPage />);

		await screen.findByText("1989");
		const filters = screen.getByRole("button", { name: "Filters" });
		expect(filters.getAttribute("aria-pressed")).toBe("false");
		fireEvent.click(filters);
		const drawer = await screen.findByRole("dialog", {
			name: "Album filters",
		});

		expect(within(drawer).getByText("Artist")).toBeTruthy();
		expect(within(drawer).getByText("Genre")).toBeTruthy();
		expect(within(drawer).getByText("1 album")).toBeTruthy();
	});

	it("keeps the header visible while showing ten square loading cards", () => {
		const pendingRequest = new Promise(() => undefined);
		mocks.listAlbums.mockReturnValue(pendingRequest);
		mocks.listArtists.mockReturnValue(pendingRequest);

		renderWithQuery(<AlbumsPage />);

		expect(screen.getByRole("heading", { name: "Albums" })).toBeTruthy();
		expect(screen.getByRole("status").textContent).toContain("Loading albums");
		expect(screen.getAllByTestId("collection-card-skeleton")).toHaveLength(10);
	});

	it("retries all album page queries from the shared error panel", async () => {
		mocks.listAlbums.mockRejectedValue(new Error("album request failed"));
		mocks.listArtists.mockRejectedValue(new Error("artist request failed"));

		renderWithQuery(<AlbumsPage />);

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain("Unable to load albums");
		expect(alert.textContent).toContain("Check your connection and try again.");
		fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));

		await waitFor(() => {
			expect(mocks.listAlbums.mock.calls.length).toBeGreaterThanOrEqual(4);
			expect(mocks.listArtists.mock.calls.length).toBeGreaterThanOrEqual(2);
		});
	});

	it("shows an album error while another page query is still pending", async () => {
		mocks.listAlbums.mockRejectedValue(new Error("album request failed"));
		mocks.listArtists.mockReturnValue(new Promise(() => undefined));

		renderWithQuery(<AlbumsPage />);

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain("Unable to load albums");
		const retryButton = within(alert).getByRole("button", {
			name: "Try again",
		});
		expect((retryButton as HTMLButtonElement).disabled).toBe(false);
		expect(screen.queryByRole("status")).toBeNull();
	});

	it("shows the empty-library copy when there are no albums", async () => {
		mocks.listAlbums.mockResolvedValue({ items: [] });

		renderWithQuery(<AlbumsPage />);

		expect(await screen.findByText("No albums yet")).toBeTruthy();
		expect(screen.getByText("Import music to get started.")).toBeTruthy();
	});
});
