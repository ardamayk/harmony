import type { RadioStation } from "@repo/api-client";
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
import { parseStationTags } from "#/lib/radio-station-tags";
import { RadioStationDetailContent } from "./-station-page";

const mocks = vi.hoisted(() => ({
	getRadioStation: vi.fn(),
	patchRadioStation: vi.fn(),
	playRadioStation: vi.fn(),
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

vi.mock("#/lib/api", () => ({
	apiClient: {
		getRadioStation: mocks.getRadioStation,
		patchRadioStation: mocks.patchRadioStation,
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

const station: RadioStation = {
	id: "s1",
	name: "Radio Paradise",
	streamUrl: "https://stream.radioparadise.com/mp3-192",
	homepageUrl: "https://radioparadise.com",
	faviconUrl: "https://radioparadise.com/favicon.ico",
	country: "United States",
	language: "english",
	tags: ["rock", "eclectic"],
	codec: "MP3",
	bitrate: 192,
	source: "radio-browser",
	externalId: "rp-main",
	isFavorite: false,
	position: 0,
	lastNowPlaying: {
		raw: "Artist - Title",
		stale: false,
	},
};

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("radio station detail route", () => {
	beforeEach(() => {
		mocks.getRadioStation.mockResolvedValue(station);
		mocks.patchRadioStation.mockResolvedValue(station);
		mocks.playRadioStation.mockClear();
		mocks.patchRadioStation.mockClear();
	});

	afterEach(() => {
		cleanup();
	});

	it("parses comma separated station tags", () => {
		expect(parseStationTags("rock, eclectic, listener-supported")).toEqual([
			"rock",
			"eclectic",
			"listener-supported",
		]);
		expect(parseStationTags(" rock, , jazz ")).toEqual(["rock", "jazz"]);
	});

	it("loads station metadata into the edit form", async () => {
		renderWithQuery(<RadioStationDetailContent stationId="s1" />);

		expect(
			await screen.findByRole("heading", { name: "Radio Paradise" }),
		).toBeTruthy();
		expect(
			await screen.findByDisplayValue("https://radioparadise.com"),
		).toBeTruthy();
		expect(screen.getByDisplayValue("rock, eclectic")).toBeTruthy();
		expect(screen.getByText("Artist - Title")).toBeTruthy();
	});

	it("saves advanced metadata edits", async () => {
		renderWithQuery(<RadioStationDetailContent stationId="s1" />);

		await screen.findByDisplayValue("rock, eclectic");

		fireEvent.change(screen.getByLabelText("Tags"), {
			target: { value: "rock, eclectic, listener-supported" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Save metadata" }));

		await waitFor(() => {
			expect(mocks.patchRadioStation).toHaveBeenCalledWith("s1", {
				name: "Radio Paradise",
				streamUrl: "https://stream.radioparadise.com/mp3-192",
				homepageUrl: "https://radioparadise.com",
				faviconUrl: "https://radioparadise.com/favicon.ico",
				country: "United States",
				language: "english",
				tags: ["rock", "eclectic", "listener-supported"],
				codec: "MP3",
				bitrate: 192,
			});
		});
	});

	it("shows a not found state when station loading fails", async () => {
		mocks.getRadioStation.mockRejectedValue(new Error("not found"));

		renderWithQuery(<RadioStationDetailContent stationId="missing" />);

		expect(await screen.findByText("Radio station not found")).toBeTruthy();
	});
});
