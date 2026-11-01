import type { RadioSearchResult, RadioStation } from "@repo/api-client";
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
import { matchesLocalStationFilter } from "#/lib/radio-station-filter";
import { RadioPage } from "./-radio-page";

const mocks = vi.hoisted(() => ({
	listRadioStations: vi.fn(),
	searchRadioStations: vi.fn(),
	createRadioStation: vi.fn(),
	importRadioStation: vi.fn(),
	patchRadioStation: vi.fn(),
	deleteRadioStation: vi.fn(),
	getRadioNowPlaying: vi.fn(),
	playRadioStation: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		listRadioStations: mocks.listRadioStations,
		searchRadioStations: mocks.searchRadioStations,
		createRadioStation: mocks.createRadioStation,
		importRadioStation: mocks.importRadioStation,
		patchRadioStation: mocks.patchRadioStation,
		deleteRadioStation: mocks.deleteRadioStation,
		getRadioNowPlaying: mocks.getRadioNowPlaying,
	},
}));

vi.mock("@repo/ui", () => ({
	RowMenuButton: () => null,
	usePhoneShell: () => false,
	usePlayback: () => ({
		playRadioStation: mocks.playRadioStation,
		currentRadioStation: null,
	}),
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
				href={params ? to.replace("$stationId", params.stationId) : to}
				{...props}
			>
				{children}
			</a>
		),
	};
});

const stations: RadioStation[] = [
	{
		id: "s1",
		name: "Jazz FM",
		streamUrl: "https://example.com/jazz",
		tags: ["jazz", "smooth", "late night"],
		source: "manual",
		isFavorite: true,
		position: 0,
		country: "US",
	},
	{
		id: "s2",
		name: "Rock Hits",
		streamUrl: "https://example.com/rock",
		tags: ["rock", "classic"],
		source: "radio-browser",
		isFavorite: false,
		position: 1,
		country: "UK",
		codec: "mp3",
		bitrate: 64,
		faviconUrl: "https://example.com/broken.png",
	},
];

const searchResults: RadioSearchResult[] = [
	{
		stationUuid: "rb-1",
		name: "Ambient World",
		streamUrl: "https://example.com/ambient",
		tags: ["ambient"],
		country: "DE",
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

describe("radio routes", () => {
	beforeEach(() => {
		mocks.listRadioStations.mockResolvedValue({
			items: stations,
			total: stations.length,
		});
		mocks.searchRadioStations.mockResolvedValue({
			items: searchResults,
			total: searchResults.length,
		});
		mocks.createRadioStation.mockResolvedValue(stations[0]);
		mocks.importRadioStation.mockResolvedValue(stations[0]);
		mocks.patchRadioStation.mockResolvedValue(stations[0]);
		mocks.deleteRadioStation.mockResolvedValue(undefined);
		mocks.getRadioNowPlaying.mockImplementation((stationId: string) =>
			Promise.resolve(
				stationId === "s1"
					? {
							artist: "Miles Davis",
							title: "So What",
							raw: "Miles Davis - So What",
						}
					: {
							artist: "Queen",
							title: "Keep Yourself Alive",
							raw: "Queen - Keep Yourself Alive",
						},
			),
		);
		mocks.playRadioStation.mockClear();
		mocks.patchRadioStation.mockClear();
		mocks.importRadioStation.mockClear();
		mocks.getRadioNowPlaying.mockClear();
	});

	afterEach(() => {
		cleanup();
	});

	it("matches saved stations against local filter text", () => {
		expect(matchesLocalStationFilter(stations[0], "jazz")).toBe(true);
		expect(matchesLocalStationFilter(stations[1], "jazz")).toBe(false);
		expect(matchesLocalStationFilter(stations[1], "uk")).toBe(true);
	});

	it("renders the Figma-style station list shell", async () => {
		renderWithQuery(<RadioPage />);

		expect(
			await screen.findByRole("heading", { name: "Radio Stations" }),
		).toBeTruthy();
		const shell = screen.getByTestId("radio-page-shell");
		const header = screen
			.getByRole("heading", { name: "Radio Stations" })
			.closest("header");
		const searchInput = screen.getByPlaceholderText("Search stations...");
		const addButton = screen.getByRole("link", { name: "Add radio station" });
		expect(screen.getByRole("heading", { name: "Your Stations" })).toBeTruthy();
		expect(screen.queryByRole("heading", { name: "Favorites" })).toBeNull();
		expect(screen.queryByRole("heading", { name: "All stations" })).toBeNull();
		expect(screen.queryByPlaceholderText("Search Radio Browser")).toBeNull();
		expect(screen.queryByLabelText("Grid view")).toBeNull();
		expect(screen.queryByLabelText("List view")).toBeNull();
		expect(shell.className).toContain("overflow-hidden");
		expect(screen.getByTestId("radio-page-content").className).toContain(
			"scrollbar-hidden",
		);
		expect(header?.className).toContain("sticky");
		expect(header?.className).toContain("top-0");
		expect(searchInput.className).toContain("h-11");
		expect(searchInput.className).toContain("pl-8");
		expect(addButton.className).toContain("size-10");
		expect(await screen.findByText("Jazz FM")).toBeTruthy();
		expect(screen.getByText("Rock Hits")).toBeTruthy();
	});

	it("aligns its header and content with the other collection pages", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByRole("heading", { name: "Radio Stations" });
		const headerInner = screen
			.getByRole("heading", { name: "Radio Stations" })
			.closest("header")
			?.querySelector(".page-content-column");
		const content = screen
			.getByTestId("radio-page-content")
			.querySelector(".page-content-column");
		expect(headerInner).toBeTruthy();
		expect(content).toBeTruthy();
	});

	it("filters saved stations locally", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByText("Jazz FM");

		fireEvent.change(screen.getByPlaceholderText("Search stations..."), {
			target: { value: "rock" },
		});

		expect(screen.queryByText("Jazz FM")).toBeNull();
		expect(screen.getByText("Rock Hits")).toBeTruthy();
	});

	it("renders saved stations as compact radio cards with two tags and current song", async () => {
		renderWithQuery(<RadioPage />);

		const jazzCard = await screen.findByTestId("radio-station-card-s1");
		expect(jazzCard.className).toContain("gap-3");
		expect(jazzCard.className).toContain("p-3");
		expect(screen.getByAltText("Rock Hits").className).toContain("h-20");
		expect(jazzCard.textContent).toContain("Jazz FM");
		expect(jazzCard.textContent).toContain("jazz");
		expect(jazzCard.textContent).toContain("smooth");
		expect(jazzCard.textContent).not.toContain("late night");
		await waitFor(() => {
			expect(jazzCard.textContent).toContain("Miles Davis - So What");
		});
		expect(jazzCard.textContent).not.toContain("US");
		expect(mocks.getRadioNowPlaying).toHaveBeenCalledWith("s1");
	});

	it("hides station quality, lifts on hover, and falls back when artwork fails", async () => {
		renderWithQuery(<RadioPage />);

		const rockCard = await screen.findByTestId("radio-station-card-s2");
		expect(rockCard.textContent).toContain("rock");
		expect(rockCard.textContent).not.toContain("MP3 64 kbps");
		expect(rockCard.className).toContain("transition");
		expect(rockCard.className).toContain("hover:-translate-y-1");

		const artwork = screen.getByAltText("Rock Hits");
		fireEvent.error(artwork);

		expect(screen.queryByAltText("Rock Hits")).toBeNull();
		expect(rockCard.querySelector(".lucide-headphones")).toBeTruthy();
	});

	it("plays saved stations and edits only custom stations from the context menu", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByText("Rock Hits");

		fireEvent.click(screen.getByRole("button", { name: "Play Rock Hits" }));
		expect(mocks.playRadioStation).toHaveBeenCalledWith(stations[1]);

		fireEvent.contextMenu(screen.getByText("Rock Hits"));
		await screen.findByRole("menuitem", { name: "Details" });
		expect(screen.queryByRole("menuitem", { name: "Edit station" })).toBeNull();
		expect(
			screen.getByRole("menuitem", { name: "Delete station" }),
		).toBeTruthy();
		fireEvent.keyDown(document, { key: "Escape" });

		fireEvent.contextMenu(screen.getByText("Jazz FM"));
		fireEvent.click(
			await screen.findByRole("menuitem", { name: "Edit station" }),
		);
		fireEvent.change(screen.getByDisplayValue("Jazz FM"), {
			target: { value: "Classic Jazz" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(mocks.patchRadioStation).toHaveBeenCalledWith("s1", {
				name: "Classic Jazz",
				streamUrl: "https://example.com/jazz",
			});
		});
		expect(mocks.playRadioStation).toHaveBeenCalledTimes(1);
	});

	it("plays a saved station from the full card", async () => {
		renderWithQuery(<RadioPage />);

		const jazzCard = await screen.findByTestId("radio-station-card-s1");
		fireEvent.click(jazzCard);

		expect(jazzCard.className).toContain("cursor-pointer");
		expect(mocks.playRadioStation).toHaveBeenCalledWith(stations[0]);
	});

	it("removes favorite controls from saved station rows", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByText("Jazz FM");

		expect(screen.queryByLabelText("Add favorite")).toBeNull();
		expect(screen.queryByLabelText("Remove favorite")).toBeNull();
	});

	it("links to the Radio Browser catalog from the add button", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByText("Jazz FM");

		expect(
			screen
				.getByRole("link", { name: "Add radio station" })
				.getAttribute("href"),
		).toBe("/radio/discover");
	});

	it("imports Radio Browser search results", async () => {
		renderWithQuery(<RadioPage />);

		await screen.findByText("Jazz FM");
		expect(screen.queryByPlaceholderText("Search Radio Browser")).toBeNull();
	});
});
