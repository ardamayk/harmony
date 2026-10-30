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
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		listAlbums: mocks.listAlbums,
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
		const albumItems = [
			{
				id: "album-1",
				title: "1989",
				artistName: "Taylor Swift",
				genres: ["Pop"],
				trackCount: 2,
			},
		];
		mocks.listAlbums.mockResolvedValue({ items: albumItems });
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
			"scrollbar-hidden",
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
		expect(screen.queryByRole("button", { name: "Filters" })).toBeNull();
		expect(screen.queryByRole("button", { name: "Scan library" })).toBeNull();
	});

	it("keeps the header visible while showing ten square loading cards", () => {
		const pendingRequest = new Promise(() => undefined);
		mocks.listAlbums.mockReturnValue(pendingRequest);

		renderWithQuery(<AlbumsPage />);

		expect(screen.getByRole("heading", { name: "Albums" })).toBeTruthy();
		expect(screen.getByRole("status").textContent).toContain("Loading albums");
		expect(screen.getAllByTestId("collection-card-skeleton")).toHaveLength(10);
	});

	it("retries the albums from the shared error panel", async () => {
		mocks.listAlbums.mockRejectedValue(new Error("album request failed"));

		renderWithQuery(<AlbumsPage />);

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain("Unable to load albums");
		expect(alert.textContent).toContain("Check your connection and try again.");
		fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));

		await waitFor(() => {
			expect(mocks.listAlbums.mock.calls.length).toBeGreaterThanOrEqual(2);
		});
	});

	it("shows the empty-library copy when there are no albums", async () => {
		mocks.listAlbums.mockResolvedValue({ items: [] });

		renderWithQuery(<AlbumsPage />);

		expect(await screen.findByText("No albums yet")).toBeTruthy();
		expect(screen.getByText("Import music to get started.")).toBeTruthy();
	});
});
