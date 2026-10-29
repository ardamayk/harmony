import {
	type AcquisitionVersion,
	ApiError,
	type ExternalTrack,
	type SoulseekSearch,
} from "@repo/api-client";
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
import { ImportSessionContext } from "#/components/import-session-context";
import { SoulseekInstallDialog } from "./soulseek-install-dialog";

const mocks = vi.hoisted(() => ({
	startSoulseekSearch: vi.fn(),
	getSoulseekSearch: vi.fn(),
	startSoulseekAcquisition: vi.fn(),
	toastSuccess: vi.fn(),
	openImport: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		startSoulseekSearch: mocks.startSoulseekSearch,
		getSoulseekSearch: mocks.getSoulseekSearch,
		startSoulseekAcquisition: mocks.startSoulseekAcquisition,
	},
}));
vi.mock("@repo/ui", async (importOriginal) => ({
	...(await importOriginal<typeof import("@repo/ui")>()),
	toast: { success: mocks.toastSuccess, error: vi.fn() },
}));

const SEARCH_ID = "00000000-0000-4000-8000-0000000000aa";

const track: ExternalTrack = {
	id: "token.signature",
	title: "One More Time (Official Video)",
	artistName: "Daft Punk",
	durationMs: 320_000,
	webpageUrl: null,
	extractor: "Youtube",
	hasArtwork: false,
};

const flac: AcquisitionVersion = {
	versionKey: "flac-24-48000",
	quality: {
		format: "flac",
		lossless: true,
		bitDepth: 24,
		sampleRateHz: 48_000,
	},
	fileCount: 19,
	totalBytes: 900 * 1024 * 1024,
	sourceCount: 2,
	completeSourceCount: 1,
	sources: [
		{
			sourceId: "source-a",
			username: "alice",
			folder: "Music\\Daft Punk\\Discovery",
			fileCount: 19,
			totalBytes: 900 * 1024 * 1024,
			complete: true,
			hasFreeUploadSlot: true,
			queueLength: 0,
			uploadSpeedBytesPerSecond: 2 * 1024 * 1024,
		},
		{
			sourceId: "source-b",
			username: "bob",
			folder: "share\\Discovery (2001)",
			fileCount: 12,
			totalBytes: 600 * 1024 * 1024,
			complete: false,
			hasFreeUploadSlot: false,
			queueLength: 4,
			uploadSpeedBytesPerSecond: 300 * 1024,
		},
	],
};

const mp3: AcquisitionVersion = {
	versionKey: "mp3-320-cbr",
	quality: {
		format: "mp3",
		lossless: false,
		bitrateKbps: 320,
		variableBitrate: false,
	},
	fileCount: 14,
	totalBytes: 150 * 1024 * 1024,
	sourceCount: 1,
	completeSourceCount: 1,
	sources: [
		{
			sourceId: "source-c",
			username: "carol",
			folder: "mp3\\Discovery",
			fileCount: 14,
			totalBytes: 150 * 1024 * 1024,
			complete: true,
			hasFreeUploadSlot: true,
			queueLength: 0,
			uploadSpeedBytesPerSecond: 1024 * 1024,
		},
	],
};

const mp3Vbr: AcquisitionVersion = {
	...mp3,
	versionKey: "mp3-vbr-245",
	quality: {
		format: "mp3",
		lossless: false,
		bitrateKbps: 320,
		variableBitrate: true,
	},
	sources: [{ ...mp3.sources[0]!, sourceId: "source-v", username: "vera" }],
};

const unknown: AcquisitionVersion = {
	...mp3,
	versionKey: "flac-x-x",
	quality: { format: "flac", lossless: true },
};

function searchResponse(
	state: SoulseekSearch["state"],
	versions: AcquisitionVersion[],
): SoulseekSearch {
	return {
		searchId: SEARCH_ID,
		query: "Daft Punk One More Time",
		scope: "album",
		state,
		versions,
	};
}

function renderDialog(onOpenChange = vi.fn(), external = track) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	render(
		<QueryClientProvider client={client}>
			<ImportSessionContext.Provider value={{ open: mocks.openImport }}>
				<SoulseekInstallDialog
					track={external}
					open
					onOpenChange={onOpenChange}
				/>
			</ImportSessionContext.Provider>
		</QueryClientProvider>,
	);
	return { onOpenChange };
}

function searchButton() {
	return screen.getByRole("button", {
		name: "Search Soulseek",
	}) as HTMLButtonElement;
}

async function chooseQuality(name: string) {
	fireEvent.click(screen.getByRole("button", { name: /^Quality/ }));
	fireEvent.click(await screen.findByRole("menuitemradio", { name }));
	await waitFor(() =>
		expect(
			screen.getByRole("button", { name: `Quality, ${name}` }),
		).toBeTruthy(),
	);
}

/** Chooses the quality, searches, and waits for the completed search. */
async function searchFor(
	versions: AcquisitionVersion[],
	quality = "FLAC 24/48",
) {
	await chooseQuality(quality);
	mocks.startSoulseekSearch.mockResolvedValue({ searchId: SEARCH_ID });
	mocks.getSoulseekSearch.mockResolvedValue(
		searchResponse("complete", versions),
	);
	fireEvent.click(searchButton());
	await screen.findByText(
		/^(\d+ sources? for |No source offers |Nothing found)/,
	);
}

describe("SoulseekInstallDialog", () => {
	beforeEach(() => window.localStorage.clear());
	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("keeps Search Soulseek disabled until a quality is chosen", async () => {
		renderDialog();
		expect(searchButton().disabled).toBe(true);
		expect(screen.getByText("Choose a quality, then search.")).toBeTruthy();

		await chooseQuality("MP3 320 kbps");

		expect(searchButton().disabled).toBe(false);
	});

	it("offers the fixed qualities by format", async () => {
		renderDialog();
		fireEvent.click(screen.getByRole("button", { name: "Quality" }));
		await screen.findByRole("menu");

		const groups = {
			"MP3 · kbps": ["128", "192", "256", "320"],
			"AAC (M4A) · kbps": ["128", "192", "256"],
			"Opus · kbps": ["96", "128", "160", "192"],
			"Ogg Vorbis · kbps": ["128", "160", "192", "256", "320"],
			"FLAC · bit / kHz": [
				"16/44.1",
				"16/48",
				"24/44.1",
				"24/48",
				"24/88.2",
				"24/96",
				"24/176.4",
				"24/192",
			],
		};
		for (const [label, values] of Object.entries(groups)) {
			const group = screen.getByRole("group", { name: label });
			expect(
				within(group)
					.getAllByRole("menuitemradio")
					.map((item) => item.textContent),
			).toEqual(values);
		}
		const wav = screen.getByRole("group", { name: "WAV · bit / kHz" });
		expect(within(wav).getAllByRole("menuitemradio")).toHaveLength(8);
		expect(
			screen.getByRole("menuitemradio", { name: "WAV 24/88.2" }),
		).toBeTruthy();
	});

	it("shows only the sources of the chosen quality", async () => {
		renderDialog();
		await searchFor([flac, mp3], "FLAC 24/48");

		expect(screen.getByText("2 sources for FLAC 24/48")).toBeTruthy();
		const sources = screen.getByRole("list", {
			name: "Sources for FLAC 24/48",
		});
		const [alice, bob] = within(sources).getAllByRole("listitem");
		expect(alice?.textContent).toContain("alice");
		expect(alice?.textContent).toContain("19 files");
		expect(alice?.textContent).toContain("Free slot · queue 0 · 2.0 MiB/s");
		expect(bob?.textContent).toContain("12/19 files");
		expect(bob?.textContent).toContain("No free slot · queue 4 · 300 KiB/s");
		expect(screen.getByRole("dialog").textContent).toContain(
			"19 files · 900.0 MiB · 2 sources (1 complete)",
		);
		expect(screen.queryByText("carol")).toBeNull();
		expect(screen.getByRole("dialog").textContent).not.toMatch(
			/best|recommended|original/i,
		);
	});

	it("never matches a variable bitrate to a constant one", async () => {
		renderDialog();
		await searchFor([mp3Vbr, mp3], "MP3 320 kbps");

		expect(screen.getByText("1 source for MP3 320 kbps")).toBeTruthy();
		expect(screen.getByText("carol")).toBeTruthy();
		expect(screen.queryByText("vera")).toBeNull();
	});

	it("lists the found qualities when none is the chosen one, and switches without searching again", async () => {
		renderDialog();
		await searchFor([flac, mp3Vbr, unknown], "MP3 320 kbps");

		expect(screen.getByText("No source offers MP3 320 kbps.")).toBeTruthy();
		const found = screen.getByRole("list", { name: "Found qualities" });
		expect(found.textContent).toContain("MP3 VBR ~320 kbps");
		expect(found.textContent).toContain("FLAC, quality unknown");
		fireEvent.click(within(found).getByRole("button", { name: "FLAC 24/48" }));

		expect(screen.getByText("2 sources for FLAC 24/48")).toBeTruthy();
		expect(
			screen.getByRole("button", { name: "Quality, FLAC 24/48" }),
		).toBeTruthy();
		expect(mocks.startSoulseekSearch).toHaveBeenCalledTimes(1);
	});

	it("filters the same search again when the quality changes", async () => {
		renderDialog();
		await searchFor([flac, mp3], "FLAC 24/48");
		await chooseQuality("MP3 320 kbps");

		expect(await screen.findByText("1 source for MP3 320 kbps")).toBeTruthy();
		expect(screen.getByText("carol")).toBeTruthy();
		expect(mocks.startSoulseekSearch).toHaveBeenCalledTimes(1);
	});

	it("remembers the last quality per scope", async () => {
		renderDialog();
		await chooseQuality("FLAC 24/88.2");
		fireEvent.click(screen.getByRole("radio", { name: "Track" }));
		await chooseQuality("MP3 256 kbps");
		cleanup();

		renderDialog();
		expect(
			screen.getByRole("button", { name: "Quality, FLAC 24/88.2" }),
		).toBeTruthy();
		fireEvent.click(screen.getByRole("radio", { name: "Track" }));
		expect(
			screen.getByRole("button", { name: "Quality, MP3 256 kbps" }),
		).toBeTruthy();
	});

	it("prefills the query without video decorations and defaults to the Album", () => {
		renderDialog();

		expect(
			(screen.getByLabelText("Soulseek query") as HTMLInputElement).value,
		).toBe("Daft Punk One More Time");
		expect(
			screen.getByRole("radio", { name: "Album" }).getAttribute("aria-checked"),
		).toBe("true");
		expect(
			screen.getByRole("radio", { name: "Track" }).getAttribute("aria-checked"),
		).toBe("false");
		expect(mocks.startSoulseekSearch).not.toHaveBeenCalled();
	});

	it("prefills the Album scope with the reported Album and the Track scope with the title", () => {
		renderDialog(vi.fn(), { ...track, album: "Discovery" });
		const query = screen.getByLabelText("Soulseek query") as HTMLInputElement;

		expect(query.value).toBe("Daft Punk Discovery");
		fireEvent.click(screen.getByRole("radio", { name: "Track" }));
		expect(query.value).toBe("Daft Punk One More Time");
		fireEvent.click(screen.getByRole("radio", { name: "Album" }));
		expect(query.value).toBe("Daft Punk Discovery");
	});

	it("keeps an edited query when the scope changes", () => {
		renderDialog(vi.fn(), { ...track, album: "Discovery" });
		const query = screen.getByLabelText("Soulseek query") as HTMLInputElement;
		fireEvent.change(query, { target: { value: "Daft Punk Alive 2007" } });
		fireEvent.click(screen.getByRole("radio", { name: "Track" }));

		expect(query.value).toBe("Daft Punk Alive 2007");
	});

	it("searches the edited query and scope", async () => {
		renderDialog();
		fireEvent.change(screen.getByLabelText("Soulseek query"), {
			target: { value: "Daft Punk Discovery" },
		});
		fireEvent.click(screen.getByRole("radio", { name: "Track" }));
		await searchFor([mp3]);

		expect(mocks.startSoulseekSearch).toHaveBeenCalledWith(
			"Daft Punk Discovery",
			"track",
		);
		expect(mocks.getSoulseekSearch).toHaveBeenCalledWith(SEARCH_ID);
	});

	it("shows matching sources while the search runs and keeps polling until complete", async () => {
		renderDialog();
		await chooseQuality("FLAC 24/48");
		mocks.startSoulseekSearch.mockResolvedValue({ searchId: SEARCH_ID });
		mocks.getSoulseekSearch
			.mockResolvedValueOnce(searchResponse("searching", [mp3]))
			.mockResolvedValueOnce(searchResponse("searching", [flac, mp3]))
			.mockResolvedValue(searchResponse("complete", [flac, mp3]));
		fireEvent.click(searchButton());

		expect(await screen.findByText("Searching Soulseek…")).toBeTruthy();
		expect(
			await screen.findByText(
				"Searching Soulseek… 2 sources for FLAC 24/48 so far",
				undefined,
				{ timeout: 3000 },
			),
		).toBeTruthy();
		await screen.findByText("2 sources for FLAC 24/48", undefined, {
			timeout: 3000,
		});
	});

	it("starts the chosen quality, then opens Import Music", async () => {
		mocks.startSoulseekAcquisition.mockResolvedValue({
			acquisitionId: "acq-1",
			query: "Daft Punk One More Time",
		});
		const { onOpenChange } = renderDialog();
		await searchFor([flac, mp3], "MP3 320 kbps");

		fireEvent.click(
			screen.getByRole("button", { name: "Install MP3 320 kbps" }),
		);

		await waitFor(() =>
			expect(mocks.startSoulseekAcquisition).toHaveBeenCalledWith({
				searchId: SEARCH_ID,
				versionKey: "mp3-320-cbr",
			}),
		);
		await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
		expect(mocks.openImport).toHaveBeenCalled();
		expect(mocks.toastSuccess).toHaveBeenCalledWith(
			"Installing “Daft Punk One More Time”. Follow it in Import Music.",
		);
	});

	it("starts from a specific source when the listener picks one", async () => {
		mocks.startSoulseekAcquisition.mockResolvedValue({
			acquisitionId: "acq-1",
			query: "q",
		});
		renderDialog();
		await searchFor([flac]);
		fireEvent.click(screen.getByRole("button", { name: "Install from bob" }));

		await waitFor(() =>
			expect(mocks.startSoulseekAcquisition).toHaveBeenCalledWith({
				searchId: SEARCH_ID,
				versionKey: "flac-24-48000",
				sourceId: "source-b",
			}),
		);
	});

	describe("when a search finds nothing", () => {
		const filtered: ExternalTrack = {
			...track,
			title: "Ariana Grande - Problem (Official Video)",
			artistName: "Ariana Grande",
		};

		async function searchForNothing() {
			await searchFor([]);
			expect(
				screen.getByText("Nothing found. Edit the query and search again."),
			).toBeTruthy();
			expect(
				screen.queryByRole("list", { name: "Found qualities" }),
			).toBeNull();
		}

		it("searches the Album without the artist", async () => {
			renderDialog(vi.fn(), { ...filtered, album: "My Everything" });
			await searchForNothing();
			expect(
				screen.getByText(/Soulseek filters some artist names/),
			).toBeTruthy();

			fireEvent.click(
				screen.getByRole("button", { name: "Search without the artist" }),
			);

			expect(
				(screen.getByLabelText("Soulseek query") as HTMLInputElement).value,
			).toBe("My Everything");
			await waitFor(() =>
				expect(mocks.startSoulseekSearch).toHaveBeenLastCalledWith(
					"My Everything",
					"album",
				),
			);
		});

		it("searches the title without the artist for the Track scope", async () => {
			renderDialog(vi.fn(), { ...filtered, album: "My Everything" });
			fireEvent.click(screen.getByRole("radio", { name: "Track" }));
			expect(
				(screen.getByLabelText("Soulseek query") as HTMLInputElement).value,
			).toBe("Ariana Grande Problem");
			await searchForNothing();

			fireEvent.click(
				screen.getByRole("button", { name: "Search without the artist" }),
			);

			await waitFor(() =>
				expect(mocks.startSoulseekSearch).toHaveBeenLastCalledWith(
					"Problem",
					"track",
				),
			);
		});

		it("offers no search without the artist when the query names none", async () => {
			renderDialog(vi.fn(), { ...filtered, album: "My Everything" });
			fireEvent.change(screen.getByLabelText("Soulseek query"), {
				target: { value: "My Everything" },
			});
			await searchForNothing();

			expect(
				screen.getByText(/Soulseek filters some artist names/),
			).toBeTruthy();
			expect(
				screen.queryByRole("button", { name: "Search without the artist" }),
			).toBeNull();
		});
	});

	it("explains an unreachable or logged-out slskd", async () => {
		renderDialog();
		await chooseQuality("FLAC 24/48");
		mocks.startSoulseekSearch.mockRejectedValueOnce(
			new ApiError(503, {
				error: "slskd unreachable",
				code: "slskd_unreachable",
				message: "slskd unreachable",
			}),
		);
		fireEvent.click(screen.getByRole("button", { name: "Search Soulseek" }));
		expect((await screen.findByRole("alert")).textContent).toBe(
			"Couldn't reach slskd. Check that it is running, then retry.",
		);

		mocks.startSoulseekSearch.mockRejectedValueOnce(
			new ApiError(503, {
				error: "not logged in",
				code: "soulseek_not_logged_in",
				message: "not logged in",
			}),
		);
		fireEvent.click(screen.getByRole("button", { name: "Search Soulseek" }));
		await waitFor(() =>
			expect(screen.getByRole("alert").textContent).toBe(
				"slskd isn't logged in to Soulseek. Check its Soulseek account, then retry.",
			),
		);
	});
});
