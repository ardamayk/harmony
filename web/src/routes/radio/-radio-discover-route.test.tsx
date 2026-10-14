import type { RadioSearchResult } from "@repo/api-client";
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
import { RadioDiscoverPage } from "./-radio-discover-page";

const mocks = vi.hoisted(() => ({
	searchRadioStations: vi.fn(),
	listRadioCatalogCountries: vi.fn(),
	listRadioCatalogTags: vi.fn(),
	importRadioStation: vi.fn(),
	playRadioCatalogPreview: vi.fn(),
}));

const FILTER_INTERACTION_TIMEOUT_MS = 20_000;

vi.mock("#/lib/api", () => ({
	apiClient: {
		searchRadioStations: mocks.searchRadioStations,
		listRadioCatalogCountries: mocks.listRadioCatalogCountries,
		listRadioCatalogTags: mocks.listRadioCatalogTags,
		importRadioStation: mocks.importRadioStation,
	},
}));

vi.mock("@repo/ui", () => ({
	usePlayback: () => ({
		playRadioCatalogPreview: mocks.playRadioCatalogPreview,
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
			...props
		}: {
			children: React.ReactNode;
			to: string;
		}) => (
			<a href={to} {...props}>
				{children}
			</a>
		),
	};
});

const catalogEntries: RadioSearchResult[] = [
	{
		stationUuid: "swiss-pop",
		name: "Radio Swiss Pop",
		streamUrl: "https://example.com/pop",
		faviconUrl: "https://example.com/pop.png",
		country: "Switzerland",
		language: "italian",
		tags: ["aac+", "pop", "public radio"],
		codec: "AAC",
		bitrate: 128,
		healthStatus: "healthy",
	},
	{
		stationUuid: "swiss-jazz",
		name: "Radio Swiss Jazz",
		streamUrl: "https://example.com/jazz",
		country: "Switzerland",
		tags: ["jazz", "public radio"],
		codec: "AAC",
		bitrate: 48,
		healthStatus: "broken",
	},
	{
		stationUuid: "china-news",
		name: "China News Radio",
		streamUrl: "https://example.com/china",
		country: "China",
		language: "chinese",
		tags: ["news", "talk", "public radio"],
		codec: "UNKNOWN",
		bitrate: 800,
		healthStatus: "healthy",
	},
	{
		stationUuid: "power-fm",
		name: "Power FM Türkiye",
		streamUrl: "https://example.com/power",
		country: "Türkiye",
		tags: ["pop"],
		codec: "UNKNOWN",
		bitrate: 0,
		healthStatus: "healthy",
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

describe("radio discover route", () => {
	beforeEach(() => {
		Element.prototype.scrollIntoView = vi.fn();
		mocks.searchRadioStations.mockResolvedValue({
			items: catalogEntries,
			total: catalogEntries.length,
		});
		mocks.listRadioCatalogCountries.mockResolvedValue({
			items: [
				{ name: "Switzerland", code: "CH", stationCount: 10 },
				{ name: "China", code: "CN", stationCount: 8 },
			],
			total: 2,
		});
		mocks.listRadioCatalogTags.mockResolvedValue({
			items: [
				{ name: "jazz", stationCount: 6 },
				{ name: "news", stationCount: 5 },
			],
			total: 2,
		});
		mocks.importRadioStation.mockResolvedValue({
			id: "local-1",
			name: "Radio Swiss Pop",
			streamUrl: "https://example.com/pop",
			tags: ["pop"],
			source: "radio-browser",
			isFavorite: false,
			position: 0,
		});
		mocks.playRadioCatalogPreview.mockClear();
		mocks.importRadioStation.mockClear();
		mocks.listRadioCatalogCountries.mockClear();
		mocks.listRadioCatalogTags.mockClear();
	});

	afterEach(() => {
		cleanup();
	});

	it("renders catalog cards with core metadata", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		expect(
			await screen.findByRole("heading", { name: "Discover Radio" }),
		).toBeTruthy();
		expect(await screen.findByText("Radio Swiss Pop")).toBeTruthy();
		const swissPop = screen.getByTestId("radio-catalog-card-swiss-pop");
		expect(screen.getByTestId("radio-catalog-grid").className).not.toContain(
			"snap-y",
		);
		expect(swissPop.className).not.toContain("snap-start");
		expect(swissPop.className).toContain("gap-4");
		expect(swissPop.className).toContain("p-4");
		expect(screen.getByAltText("Radio Swiss Pop").className).toContain("h-24");
		expect(swissPop.className).toContain("items-start");
		expect(
			within(swissPop).getByRole("heading", { name: "Radio Swiss Pop" })
				.className,
		).toContain("truncate");
		expect(
			within(swissPop).getByRole("heading", { name: "Radio Swiss Pop" })
				.className,
		).toContain("text-base");
		expect(
			within(swissPop).getByRole("heading", { name: "Radio Swiss Pop" })
				.className,
		).not.toContain("line-clamp-2");
		expect(screen.getAllByText("Switzerland")[0]).toBeTruthy();
		expect(screen.queryByText("Switzerland / italian")).toBeNull();
		expect(screen.getByText("AAC 128 kbps")).toBeTruthy();
		expect(screen.getByText("800 kbps")).toBeTruthy();
		expect(screen.queryByText("UNKNOWN 800 kbps")).toBeNull();
		expect(within(swissPop).queryByText("•")).toBeNull();
		expect(screen.getAllByText("Healthy")[0]).toBeTruthy();
		expect(screen.getByText("Quality unavailable")).toBeTruthy();
		expect(within(swissPop).getByText("aac+")).toBeTruthy();
		expect(within(swissPop).getByText("pop")).toBeTruthy();
		expect(within(swissPop).queryByText("public radio")).toBeNull();
		expect(swissPop.className).toContain("duration-300");
		expect(swissPop.className).toContain("ease-out");
		expect(swissPop.className).toContain("hover:-translate-y-1");
		expect(swissPop.className).toContain("hover:shadow-lg");
	});

	it(
		"searches and filters the catalog from dropdown options",
		async () => {
			renderWithQuery(<RadioDiscoverPage />);

			await screen.findByText("Radio Swiss Pop");
			fireEvent.change(screen.getByPlaceholderText("Search station name..."), {
				target: { value: "jazz" },
			});
			fireEvent.click(screen.getByRole("button", { name: "Filters" }));
			const drawer = await screen.findByRole("dialog", {
				name: "Radio catalog filters",
			});
			fireEvent.click(within(drawer).getByRole("button", { name: "Country" }));
			fireEvent.click(
				await screen.findByRole("option", { name: /Switzerland/ }),
			);
			fireEvent.click(within(drawer).getByRole("button", { name: "Genre" }));
			fireEvent.click(await screen.findByRole("option", { name: "jazz" }));
			fireEvent.click(within(drawer).getByRole("radio", { name: "MP3" }));
			fireEvent.click(within(drawer).getByRole("radio", { name: "128 kbps+" }));

			await waitFor(() => {
				expect(mocks.searchRadioStations).toHaveBeenLastCalledWith(
					expect.objectContaining({
						q: "jazz",
						country: "Switzerland",
						tag: "jazz",
						codec: "MP3",
						minBitrate: 128,
						limit: 40,
						offset: 0,
					}),
				);
			});
		},
		FILTER_INTERACTION_TIMEOUT_MS,
	);

	it(
		"resets catalog filters from the drawer",
		async () => {
			renderWithQuery(<RadioDiscoverPage />);

			await screen.findByText("Radio Swiss Pop");
			fireEvent.change(screen.getByPlaceholderText("Search station name..."), {
				target: { value: "jazz" },
			});
			fireEvent.click(screen.getByRole("button", { name: "Filters" }));
			const drawer = await screen.findByRole("dialog", {
				name: "Radio catalog filters",
			});
			fireEvent.click(
				within(drawer).getByRole("button", { name: "List view" }),
			);
			expect(screen.getByTestId("radio-catalog-list")).toBeTruthy();
			fireEvent.click(within(drawer).getByRole("button", { name: "Country" }));
			fireEvent.click(
				await screen.findByRole("option", { name: /Switzerland/ }),
			);
			fireEvent.click(within(drawer).getByRole("button", { name: "Genre" }));
			fireEvent.click(await screen.findByRole("option", { name: "jazz" }));
			fireEvent.click(within(drawer).getByRole("radio", { name: "MP3" }));
			fireEvent.click(within(drawer).getByRole("radio", { name: "128 kbps+" }));

			await waitFor(() => {
				expect(mocks.searchRadioStations).toHaveBeenLastCalledWith(
					expect.objectContaining({
						q: "jazz",
						country: "Switzerland",
						tag: "jazz",
						codec: "MP3",
						minBitrate: 128,
					}),
				);
			});

			fireEvent.click(
				within(drawer).getByRole("button", { name: "Reset filters" }),
			);

			await waitFor(() => {
				expect(
					(
						screen.getByPlaceholderText(
							"Search station name...",
						) as HTMLInputElement
					).value,
				).toBe("jazz");
				expect(mocks.searchRadioStations).toHaveBeenLastCalledWith(
					expect.objectContaining({
						q: "jazz",
						country: undefined,
						tag: undefined,
						codec: undefined,
						minBitrate: undefined,
						limit: 40,
						offset: 0,
					}),
				);
			});
			expect(
				within(drawer)
					.getByRole("button", { name: "List view" })
					.getAttribute("aria-pressed"),
			).toBe("true");
			expect(
				(
					within(drawer).getByRole("radio", {
						name: "Any format",
					}) as HTMLInputElement
				).checked,
			).toBe(true);
			expect(
				(
					within(drawer).getByRole("radio", {
						name: "Any bitrate",
					}) as HTMLInputElement
				).checked,
			).toBe(true);
		},
		FILTER_INTERACTION_TIMEOUT_MS,
	);

	it(
		"renders static country and genre filter options",
		async () => {
			renderWithQuery(<RadioDiscoverPage />);

			await screen.findByText("Radio Swiss Pop");
			fireEvent.click(screen.getByRole("button", { name: "Filters" }));
			const drawer = await screen.findByRole("dialog", {
				name: "Radio catalog filters",
			});
			fireEvent.click(within(drawer).getByRole("button", { name: "Country" }));
			expect(screen.getByRole("option", { name: /Switzerland/ })).toBeTruthy();
			fireEvent.click(screen.getByRole("option", { name: "All countries" }));
			fireEvent.click(within(drawer).getByRole("button", { name: "Genre" }));
			expect(screen.getByRole("option", { name: "rock" })).toBeTruthy();
		},
		FILTER_INTERACTION_TIMEOUT_MS,
	);

	it(
		"filters country options inside the dropdown",
		async () => {
			renderWithQuery(<RadioDiscoverPage />);

			await screen.findByText("Radio Swiss Pop");
			fireEvent.click(screen.getByRole("button", { name: "Filters" }));
			const drawer = await screen.findByRole("dialog", {
				name: "Radio catalog filters",
			});
			fireEvent.click(within(drawer).getByRole("button", { name: "Country" }));
			expect(screen.getByRole("option", { name: /Afghanistan/ })).toBeTruthy();

			fireEvent.change(screen.getByPlaceholderText("Search countries..."), {
				target: { value: "switz" },
			});

			expect(screen.getByRole("option", { name: /Switzerland/ })).toBeTruthy();
			expect(screen.queryByRole("option", { name: /Afghanistan/ })).toBeNull();
		},
		FILTER_INTERACTION_TIMEOUT_MS,
	);

	it(
		"filters genre options inside the dropdown",
		async () => {
			renderWithQuery(<RadioDiscoverPage />);

			await screen.findByText("Radio Swiss Pop");
			fireEvent.click(screen.getByRole("button", { name: "Filters" }));
			const drawer = await screen.findByRole("dialog", {
				name: "Radio catalog filters",
			});
			fireEvent.click(within(drawer).getByRole("button", { name: "Genre" }));
			expect(screen.getByRole("option", { name: "rock" })).toBeTruthy();

			fireEvent.change(screen.getByPlaceholderText("Search genres..."), {
				target: { value: "jazz" },
			});

			expect(screen.getByRole("option", { name: "jazz" })).toBeTruthy();
			expect(screen.queryByRole("option", { name: "rock" })).toBeNull();
		},
		FILTER_INTERACTION_TIMEOUT_MS,
	);

	it("renders a compact header without notification or user controls", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		await screen.findByText("Radio Swiss Pop");
		const header = screen
			.getByRole("heading", { name: "Discover Radio" })
			.closest("header");
		const searchInput = screen.getByPlaceholderText("Search station name...");
		const filterButton = screen.getByRole("button", { name: "Filters" });
		const description = screen.getByText(
			"Browse Radio Browser catalog entries A-Z.",
		);

		expect(screen.queryByRole("button", { name: "Notifications" })).toBeNull();
		expect(screen.queryByRole("link", { name: "Your Stations" })).toBeNull();
		expect(header?.textContent).not.toContain("User");
		expect(header?.className).toContain("sticky");
		expect(header?.className).toContain("top-0");
		expect(header?.className).toContain("py-3");
		expect(header?.firstElementChild?.className).toContain("gap-2");
		expect(searchInput.className).toContain("h-11");
		expect(searchInput.className).toContain("rounded-xl");
		expect(filterButton.className).toContain("size-10");
		expect(description.parentElement?.className).toContain("items-center");
		expect(description.className).toContain("text-xs");
		expect(screen.queryByRole("group", { name: "Catalog layout" })).toBeNull();

		fireEvent.click(filterButton);
		const drawer = await screen.findByRole("dialog", {
			name: "Radio catalog filters",
		});
		expect(
			within(drawer).getByRole("group", { name: "Catalog layout" }),
		).toBeTruthy();
	});

	it("keeps filter dropdowns above catalog cards", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		await screen.findByText("Radio Swiss Pop");
		const header = screen
			.getByRole("heading", { name: "Discover Radio" })
			.closest("header");

		expect(header?.className).toContain("z-");
	});

	it("falls back when a catalog artwork asset is too small", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		const artwork = (await screen.findByAltText(
			"Radio Swiss Pop",
		)) as HTMLImageElement;
		Object.defineProperties(artwork, {
			naturalWidth: { value: 24, configurable: true },
			naturalHeight: { value: 24, configurable: true },
		});
		fireEvent.load(artwork);

		expect(screen.queryByAltText("Radio Swiss Pop")).toBeNull();
	});

	it("uses left click for live playback and right click menu for import and details", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		await screen.findByText("Radio Swiss Jazz");

		expect(screen.queryByRole("button", { name: "Play preview" })).toBeNull();
		expect(screen.queryByRole("button", { name: "Import" })).toBeNull();

		fireEvent.click(screen.getByTestId("radio-catalog-card-swiss-jazz"));
		expect(mocks.playRadioCatalogPreview).toHaveBeenCalledWith(
			catalogEntries[1],
		);

		fireEvent.contextMenu(screen.getByTestId("radio-catalog-card-swiss-jazz"));
		fireEvent.click(await screen.findByRole("menuitem", { name: /Details/ }));
		const dialog = await screen.findByRole("dialog", {
			name: "Radio Swiss Jazz",
		});
		expect(within(dialog).getByText("public radio")).toBeTruthy();

		fireEvent.keyDown(document, { key: "Escape" });
		fireEvent.contextMenu(screen.getByTestId("radio-catalog-card-swiss-jazz"));
		fireEvent.click(
			await screen.findByRole("menuitem", {
				name: /Import \/ Add to radio stations/,
			}),
		);
		await waitFor(() => {
			expect(mocks.importRadioStation).toHaveBeenCalledWith({
				result: catalogEntries[1],
			});
		});
	});

	it("shows live playback failure copy without preview wording", async () => {
		mocks.playRadioCatalogPreview.mockRejectedValueOnce(new Error("failed"));
		renderWithQuery(<RadioDiscoverPage />);

		await screen.findByText("Radio Swiss Jazz");
		fireEvent.click(screen.getByTestId("radio-catalog-card-swiss-jazz"));

		expect(await screen.findByText("Live playback unavailable.")).toBeTruthy();
		expect(screen.queryByText(/preview/i)).toBeNull();
	});

	it("switches between compact card and list views", async () => {
		renderWithQuery(<RadioDiscoverPage />);

		await screen.findByText("Radio Swiss Pop");
		expect(screen.getByTestId("radio-catalog-grid")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Filters" }));
		const drawer = await screen.findByRole("dialog", {
			name: "Radio catalog filters",
		});
		fireEvent.click(within(drawer).getByRole("button", { name: "List view" }));
		expect(screen.getByTestId("radio-catalog-list")).toBeTruthy();
		expect(
			screen.getByTestId("radio-catalog-card-swiss-pop").className,
		).toContain("duration-300");
		expect(
			screen.getByTestId("radio-catalog-card-swiss-pop").className,
		).toContain("ease-out");
		expect(
			screen.getByTestId("radio-catalog-card-swiss-pop").className,
		).toContain("hover:-translate-y-1");
		expect(
			screen.getByTestId("radio-catalog-card-swiss-pop").className,
		).toContain("hover:shadow-lg");
	});
});
