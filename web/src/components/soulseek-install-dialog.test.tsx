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
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
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

const unknown: AcquisitionVersion = {
	...mp3,
	versionKey: "flac-unknown",
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

async function searchFor(versions: AcquisitionVersion[]) {
	mocks.startSoulseekSearch.mockResolvedValue({ searchId: SEARCH_ID });
	mocks.getSoulseekSearch.mockResolvedValue(
		searchResponse("complete", versions),
	);
	fireEvent.click(screen.getByRole("button", { name: "Search Soulseek" }));
	await screen.findByText(
		`${versions.length} ${versions.length === 1 ? "version" : "versions"} found`,
	);
}

describe("SoulseekInstallDialog", () => {
	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
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

	it("shows versions while the search runs and keeps polling until complete", async () => {
		mocks.startSoulseekSearch.mockResolvedValue({ searchId: SEARCH_ID });
		mocks.getSoulseekSearch
			.mockResolvedValueOnce(searchResponse("searching", [flac]))
			.mockResolvedValue(searchResponse("complete", [flac, mp3]));
		renderDialog();
		fireEvent.click(screen.getByRole("button", { name: "Search Soulseek" }));

		expect(
			await screen.findByText("Searching Soulseek… 1 version so far"),
		).toBeTruthy();
		expect(
			screen.getByRole("listitem", { name: "FLAC 24-bit / 48 kHz" }),
		).toBeTruthy();
		await screen.findByText("2 versions found", undefined, { timeout: 3000 });
		expect(screen.getByRole("listitem", { name: "MP3 320 kbps" })).toBeTruthy();
	});

	it("shows each version's quality, counts and incomplete sources, never a best one", async () => {
		renderDialog();
		await searchFor([flac, mp3, unknown]);

		const flacRow = screen.getByRole("listitem", {
			name: "FLAC 24-bit / 48 kHz",
		});
		expect(flacRow.textContent).toContain(
			"19 files · 900.0 MiB · 2 sources (1 complete)",
		);
		expect(
			screen.getByRole("listitem", { name: "FLAC, quality unknown" }),
		).toBeTruthy();

		fireEvent.click(
			within(flacRow).getByRole("button", {
				name: "Sources for FLAC 24-bit / 48 kHz",
			}),
		);
		const sources = within(flacRow).getByRole("list", {
			name: "Sources for FLAC 24-bit / 48 kHz",
		});
		const [alice, bob] = within(sources).getAllByRole("listitem");
		expect(alice?.textContent).toContain("alice");
		expect(alice?.textContent).toContain("19 files");
		expect(alice?.textContent).toContain("Free slot · queue 0 · 2.0 MiB/s");
		expect(bob?.textContent).toContain("12/19 files");
		expect(bob?.textContent).toContain("No free slot · queue 4 · 300 KiB/s");

		const dialog = screen.getByRole("dialog");
		expect(dialog.textContent).not.toMatch(/best|recommended|original/i);
	});

	it("starts the chosen version, then opens Import Music", async () => {
		mocks.startSoulseekAcquisition.mockResolvedValue({
			acquisitionId: "acq-1",
			query: "Daft Punk One More Time",
		});
		const { onOpenChange } = renderDialog();
		await searchFor([flac, mp3]);

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
		fireEvent.click(
			screen.getByRole("button", { name: "Sources for FLAC 24-bit / 48 kHz" }),
		);
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
			mocks.startSoulseekSearch.mockResolvedValue({ searchId: SEARCH_ID });
			mocks.getSoulseekSearch.mockResolvedValue(searchResponse("complete", []));
			fireEvent.click(screen.getByRole("button", { name: "Search Soulseek" }));
			await screen.findByText(
				"No versions found. Edit the query and search again.",
			);
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
