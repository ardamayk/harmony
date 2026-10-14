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
import { ArtistsPage } from "./-artists-page";

const mocks = vi.hoisted(() => ({
	listArtists: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		listArtists: mocks.listArtists,
	},
}));

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("artists route", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		mocks.listArtists.mockResolvedValue({
			items: [{ id: "artist-1", name: "Nina Simone", albumCount: 7 }],
		});
	});

	afterEach(cleanup);

	it("aligns the header and artist cards to the shared collection width", async () => {
		renderWithQuery(<ArtistsPage />);

		await screen.findByText("Nina Simone");
		const header = screen
			.getByRole("heading", { name: "Artists" })
			.closest("header");
		expect(header?.querySelector(".page-content-column")).toBeTruthy();
		expect(
			screen
				.getByTestId("artists-page-content")
				.querySelector(".page-content-column"),
		).toBeTruthy();
	});

	it("keeps the header visible while showing the shared loading grid", () => {
		mocks.listArtists.mockReturnValue(new Promise(() => undefined));

		renderWithQuery(<ArtistsPage />);

		expect(screen.getByRole("heading", { name: "Artists" })).toBeTruthy();
		expect(screen.getByRole("status").textContent).toContain("Loading artists");
		expect(screen.getAllByTestId("collection-card-skeleton")).toHaveLength(10);
	});

	it("retries the artist query from the shared error panel", async () => {
		mocks.listArtists.mockRejectedValue(new Error("artist request failed"));

		renderWithQuery(<ArtistsPage />);

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain("Unable to load artists");
		fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
		await waitFor(() => {
			expect(mocks.listArtists.mock.calls.length).toBeGreaterThanOrEqual(2);
		});
	});

	it("uses different empty copy with and without a search from the URL", async () => {
		mocks.listArtists.mockResolvedValue({ items: [] });
		const firstRender = renderWithQuery(<ArtistsPage />);

		expect(await screen.findByText("No artists yet")).toBeTruthy();
		expect(screen.getByText("Import music to get started.")).toBeTruthy();
		firstRender.unmount();

		mocks.listArtists.mockResolvedValue({ items: [] });
		renderWithQuery(<ArtistsPage initialSearch="unmatched" />);

		expect(
			await screen.findByText("No artists match your search"),
		).toBeTruthy();
		expect(screen.getByText("Try adjusting your search.")).toBeTruthy();
	});
});
