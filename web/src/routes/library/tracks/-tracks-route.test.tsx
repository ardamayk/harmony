import { ApiError } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	createMemoryHistory,
	createRootRoute,
	createRouter,
	RouterProvider,
} from "@tanstack/react-router";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { useState } from "react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { ImportSessionProvider } from "#/components/import-session-provider";
import { TracksPage } from "./-tracks-page";
import { Route } from "./index";

const mocks = vi.hoisted(() => ({
	getHealth: vi.fn(),
	listTracks: vi.fn(),
	listPlaylists: vi.fn(),
	getPlaylist: vi.fn(),
	addPlaylistTrack: vi.fn(),
	removePlaylistTrack: vi.fn(),
	createManagedImportBatch: vi.fn(),
	getManagedImportBatch: vi.fn(),
	heartbeatManagedImportBatch: vi.fn().mockResolvedValue(undefined),
	confirmManagedImportBatch: vi.fn(),
	cancelManagedImportBatch: vi.fn(),
	cancelManagedImport: vi.fn(),
	createManagedImportJob: vi.fn(),
	uploadManagedImportFile: vi.fn(),
	confirmManagedImport: vi.fn(),
	isDesktopClient: vi.fn(),
	selectDesktopImportFiles: vi.fn(),
	selectDesktopImportFolder: vi.fn(),
	desktopUploadImportFile: vi.fn(),
	releaseDesktopImportSelections: vi.fn(),
}));

const libraryTracks = [
	{
		id: "t1",
		title: "Anti-Hero",
		artistName: "Taylor Swift",
		albumId: "a1",
		durationMs: 200_000,
		format: "flac",
		genre: "Synthpop",
	},
	{
		id: "t2",
		title: "Bad Blood",
		artistName: "Taylor Swift",
		albumId: "a2",
		durationMs: 211_000,
		format: "flac",
		genre: "Pop",
	},
];

vi.mock("#/lib/api", () => ({
	apiClient: {
		getHealth: mocks.getHealth,
		listTracks: mocks.listTracks,
		listPlaylists: mocks.listPlaylists,
		getPlaylist: mocks.getPlaylist,
		addPlaylistTrack: mocks.addPlaylistTrack,
		removePlaylistTrack: mocks.removePlaylistTrack,
		createManagedImportBatch: mocks.createManagedImportBatch,
		getManagedImportBatch: mocks.getManagedImportBatch,
		heartbeatManagedImportBatch: mocks.heartbeatManagedImportBatch,
		confirmManagedImportBatch: mocks.confirmManagedImportBatch,
		cancelManagedImportBatch: mocks.cancelManagedImportBatch,
		cancelManagedImport: mocks.cancelManagedImport,
		createManagedImportJob: mocks.createManagedImportJob,
		uploadManagedImportFile: mocks.uploadManagedImportFile,
		confirmManagedImport: mocks.confirmManagedImport,
	},
}));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
	...(await importOriginal<typeof import("@tanstack/react-router")>()),
	Link: ({
		to,
		children,
		className,
	}: {
		to: string;
		children: React.ReactNode;
		className?: string;
	}) => (
		<a href={to} className={className}>
			{children}
		</a>
	),
}));

vi.mock("#/desktop/bridge", () => ({
	isDesktopClient: mocks.isDesktopClient,
	selectDesktopImportFiles: mocks.selectDesktopImportFiles,
	selectDesktopImportFolder: mocks.selectDesktopImportFolder,
	desktopUploadImportFile: mocks.desktopUploadImportFile,
	releaseDesktopImportSelections: mocks.releaseDesktopImportSelections,
}));

vi.mock("#/components/track-list", () => ({
	TrackList: ({
		tracks,
		numbering,
		showFavorite,
		compact,
	}: {
		tracks: Array<{ id: string; title: string }>;
		numbering?: string;
		showFavorite?: boolean;
		compact?: boolean;
	}) => (
		<div>
			<p data-testid="track-numbering">{numbering}</p>
			<p data-testid="track-show-favorite">{String(showFavorite)}</p>
			<p data-testid="track-compact">{String(compact)}</p>
			{tracks.map((track) => (
				<p key={track.id}>{track.title}</p>
			))}
		</div>
	),
}));

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<ImportSessionProvider>{ui}</ImportSessionProvider>
		</QueryClientProvider>,
	);
}

async function openImportMusicDialog() {
	renderWithQuery(<TracksPage />);
	await screen.findByText("Anti-Hero");
	fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
}

function selectAudioFolder(files: File[]) {
	fireEvent.change(screen.getByLabelText("Audio folder"), {
		target: { files },
	});
}

function createFolderFile(
	contents: string,
	name: string,
	clientPath: string,
	type = "audio/flac",
) {
	const file = new File([contents], name, { type });
	Object.defineProperty(file, "webkitRelativePath", { value: clientPath });
	return file;
}

function selectFolderWithIgnoredFiles() {
	const track = createFolderFile(
		"flac bytes",
		"track.FLAC",
		"Collection/Disc 1/track.FLAC",
	);
	const hiddenTrack = createFolderFile(
		"hidden",
		"hidden.mp3",
		"Collection/.archive/hidden.mp3",
		"audio/mpeg",
	);
	const sidecar = createFolderFile(
		"cover",
		"cover.jpg",
		"Collection/Disc 1/cover.jpg",
		"image/jpeg",
	);
	selectAudioFolder([track, hiddenTrack, sidecar]);
	return track;
}

function createImportPreview(jobId: string) {
	return {
		jobId,
		status: "awaiting_confirmation",
		revision: 2,
		file: {
			originalFilename: `${jobId}.flac`,
			albumKey: "album-key",
			contentSha256: jobId,
			format: "flac",
			discNo: 1,
			trackNo:
				jobId === "import-2" ? 4 : jobId === "remaster-selection" ? 5 : 3,
			albumArtists: ["Test Album Artist"],
			title: jobId,
			artists: ["Test Artist"],
			album: "Strict Import Tests",
		},
	};
}

function createBatchFile(jobId: string, isAccepted: boolean) {
	return {
		jobId,
		state: isAccepted ? "accepted" : "unresolved",
		status: isAccepted ? "awaiting_confirmation" : "uploading",
		revision: isAccepted ? 2 : 1,
		validationProgress: isAccepted ? 100 : 0,
		selected: isAccepted,
	};
}

function mockClientFileJobs() {
	mocks.createManagedImportJob
		.mockReset()
		.mockImplementation((_batchId, clientFileId) =>
			Promise.resolve({ id: clientFileId, status: "uploading", revision: 1 }),
		);
}

function mockDeferredUploads() {
	const releases: Array<() => void> = [];
	let activeUploads = 0;
	let maximumActiveUploads = 0;
	mocks.uploadManagedImportFile.mockImplementation(async (jobId) => {
		activeUploads++;
		maximumActiveUploads = Math.max(maximumActiveUploads, activeUploads);
		await new Promise<void>((resolve) => releases.push(resolve));
		activeUploads--;
		return createImportPreview(jobId);
	});
	return {
		releases,
		getActiveUploads: () => activeUploads,
		getMaximumActiveUploads: () => maximumActiveUploads,
	};
}

function mockRetryableBatchResponses() {
	mocks.getManagedImportBatch.mockImplementation(async () => {
		const attempts = mocks.uploadManagedImportFile.mock.calls.filter(
			([jobId]) => jobId === "import-1",
		).length;
		return {
			id: "batch-1",
			status: "uploading",
			revision: 3,
			files: [
				createBatchFile("import-1", attempts > 1),
				createBatchFile("import-2", true),
			],
		};
	});
}

function mockInterruptedFolderUpload() {
	let interruptedAttempts = 0;
	let finishRetry: (() => void) | undefined;
	mocks.uploadManagedImportFile.mockImplementation(
		(jobId, _filename, _file, onProgress) => {
			if (jobId !== "import-1")
				return Promise.resolve(createImportPreview(jobId));
			interruptedAttempts++;
			if (interruptedAttempts === 1)
				return Promise.reject(new Error("upload interrupted"));
			onProgress?.(45);
			return new Promise((resolve) => {
				finishRetry = () => resolve(createImportPreview(jobId));
			});
		},
	);
	return () => finishRetry?.();
}

function expectUploadAttempts(jobId: string, count: number) {
	expect(
		mocks.uploadManagedImportFile.mock.calls.filter(
			([currentJobId]) => currentJobId === jobId,
		),
	).toHaveLength(count);
}

describe("tracks route", () => {
	it("releases retry slots for queued files and cancels pending retries", async () => {
		mocks.createManagedImportJob
			.mockReset()
			.mockImplementation(async (_batchId, clientFileId) => ({
				id: clientFileId,
				status: "uploading",
				revision: 1,
			}));
		mocks.getManagedImportBatch.mockImplementation(async () => ({
			id: "batch-1",
			status: "uploading",
			revision: 4,
			files: mocks.createManagedImportJob.mock.calls.map(([, jobId]) => ({
				jobId,
				status: "uploading",
				state: "unresolved",
				revision: 1,
				validationProgress: 0,
				selected: false,
				errorCode: "upload_interrupted",
			})),
		}));
		mocks.uploadManagedImportFile.mockImplementation(
			(_jobId, filename, _file, _onProgress, signal) => {
				if (filename === "first.flac")
					return Promise.reject(new TypeError("Connection lost"));
				return new Promise((_resolve, reject) =>
					signal.addEventListener("abort", () => reject(signal.reason), {
						once: true,
					}),
				);
			},
		);
		vi.spyOn(window, "confirm").mockReturnValue(true);
		await openImportMusicDialog();
		vi.useFakeTimers();
		await act(async () =>
			selectAudioFolder(
				["first", "second", "third"].map(
					(name) => new File(["audio"], `${name}.flac`),
				),
			),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(3);
		await act(async () =>
			fireEvent.click(screen.getByRole("button", { name: "Cancel" })),
		);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(60000);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(3);
		expect(mocks.cancelManagedImportBatch).toHaveBeenCalledTimes(1);
		expect(screen.queryByRole("dialog", { name: "Import Music" })).toBeNull();
	});

	it("keeps the import available across page navigation and renews its staging lease", async () => {
		function Navigation() {
			const [isTracksPage, setIsTracksPage] = useState(true);
			return (
				<>
					<button type="button" onClick={() => setIsTracksPage(!isTracksPage)}>
						Navigate
					</button>
					{isTracksPage ? <TracksPage /> : <p>Another page</p>}
				</>
			);
		}
		renderWithQuery(<Navigation />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		mocks.uploadManagedImportFile.mockImplementation(
			() => new Promise(() => {}),
		);
		vi.useFakeTimers();
		await act(async () =>
			selectAudioFolder([new File(["audio"], "first.flac")]),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(1);
		fireEvent.click(screen.getByRole("button", { name: "Close Import Music" }));
		fireEvent.click(screen.getByRole("button", { name: "Navigate" }));
		expect(screen.getByText("Another page")).toBeTruthy();
		fireEvent.click(
			screen.getByRole("button", { name: "Open current import" }),
		);
		expect(screen.getByRole("dialog", { name: "Import Music" })).toBeTruthy();
		await act(async () => {
			await vi.advanceTimersByTimeAsync(60000);
		});
		expect(mocks.heartbeatManagedImportBatch).toHaveBeenCalledWith("batch-1");
		expect(mocks.cancelManagedImportBatch).not.toHaveBeenCalled();
	});

	it("retries interrupted transfers after 2, 5, and 10 seconds and restarts only on manual retry", async () => {
		mocks.uploadManagedImportFile.mockRejectedValue(
			new TypeError("Network disconnected"),
		);
		mocks.getManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 2,
			files: [
				{
					jobId: "import-1",
					state: "unresolved",
					status: "uploading",
					revision: 1,
					validationProgress: 0,
					selected: false,
					errorCode: "upload_interrupted",
				},
			],
		});
		await openImportMusicDialog();
		vi.useFakeTimers();
		await act(async () =>
			selectAudioFolder([new File(["audio"], "first.flac")]),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(1);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(1999);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(1);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(1);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(2);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(5000);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(3);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(10000);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(4);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(20000);
		});
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(4);
		await act(async () =>
			fireEvent.click(
				screen.getByRole("button", { name: "Retry failed uploads" }),
			),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(5);
	});

	it("minimizes an active import and reopens it without canceling", async () => {
		mocks.uploadManagedImportFile.mockImplementation(
			() => new Promise(() => {}),
		);
		await openImportMusicDialog();
		selectAudioFolder([new File(["audio"], "first.flac")]);
		await waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(1),
		);
		fireEvent.click(screen.getByRole("button", { name: "Close Import Music" }));
		await waitFor(() =>
			expect(screen.queryByRole("dialog", { name: "Import Music" })).toBeNull(),
		);
		expect(mocks.cancelManagedImportBatch).not.toHaveBeenCalled();
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		expect(screen.getByRole("dialog", { name: "Import Music" })).toBeTruthy();
		expect(mocks.createManagedImportBatch).toHaveBeenCalledTimes(1);
	});

	beforeEach(() => {
		mocks.heartbeatManagedImportBatch.mockClear();
		mocks.getHealth.mockReset();
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "test",
			capabilities: [
				"api.v1",
				"managed-import.v1",
				"managed-import-batches.v1",
			],
		});
		mocks.listTracks.mockReset();
		mocks.createManagedImportJob.mockReset();
		mocks.createManagedImportBatch.mockReset();
		mocks.getManagedImportBatch.mockReset();
		mocks.confirmManagedImportBatch.mockReset();
		mocks.cancelManagedImportBatch.mockReset();
		mocks.uploadManagedImportFile.mockReset();
		mocks.confirmManagedImport.mockReset();
		mocks.isDesktopClient.mockReset().mockReturnValue(false);
		mocks.selectDesktopImportFiles.mockReset();
		mocks.selectDesktopImportFolder.mockReset();
		mocks.desktopUploadImportFile.mockReset();
		mocks.releaseDesktopImportSelections
			.mockReset()
			.mockResolvedValue(undefined);
		mocks.listTracks.mockResolvedValue({
			items: libraryTracks,
		});
		mocks.listPlaylists.mockResolvedValue({
			items: [{ id: "favorites", name: "Favorites", isDefault: true }],
		});
		mocks.getPlaylist.mockResolvedValue({ tracks: [] });
		mocks.addPlaylistTrack.mockResolvedValue({ tracks: [] });
		mocks.removePlaylistTrack.mockResolvedValue({ tracks: [] });
		mocks.createManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 1,
			files: [],
		});
		mocks.createManagedImportJob
			.mockResolvedValueOnce({
				id: "import-1",
				status: "uploading",
				revision: 1,
			})
			.mockResolvedValueOnce({
				id: "import-2",
				status: "uploading",
				revision: 1,
			});
		mocks.uploadManagedImportFile.mockResolvedValue({
			jobId: "import-1",
			status: "awaiting_confirmation",
			revision: 2,
			file: {
				originalFilename: "strict-import.flac",
				title: "Inspection Fixture",
				artists: ["Test Artist"],
				albumArtists: ["Test Album Artist"],
				album: "Strict Import Tests",
				genres: ["Electronic"],
				trackNo: 3,
				discNo: 1,
				durationMs: 250,
				format: "flac",
				artworkMediaType: "image/png",
			},
		});
		mocks.confirmManagedImport.mockResolvedValue({
			jobId: "import-1",
			status: "committed",
			revision: 3,
			trackId: "imported-track",
		});
		mocks.getManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 3,
			files: [
				{
					jobId: "import-1",
					state: "accepted",
					status: "awaiting_confirmation",
					revision: 2,
					validationProgress: 100,
					selected: true,
				},
				{
					jobId: "import-2",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					originalFilename: "broken.flac",
					selected: false,
					errorCode: "invalid_metadata",
					errorReason: "TITLE is required",
				},
			],
		});
		mocks.confirmManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "completed",
			revision: 5,
			files: [
				{
					jobId: "import-1",
					state: "completed",
					status: "committed",
					revision: 3,
					validationProgress: 100,
					selected: true,
					outcome: "imported",
					trackId: "imported-track",
				},
				{
					jobId: "import-2",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					selected: false,
					outcome: "rejected",
					errorCode: "invalid_metadata",
					errorReason: "TITLE is required",
				},
			],
		});
		mocks.cancelManagedImportBatch.mockResolvedValue(undefined);
	});

	afterEach(() => {
		vi.useRealTimers();
		Reflect.deleteProperty(window, "earthlyDesktop");
		vi.useRealTimers();
		cleanup();
	});

	it("restores focus to the Import Music action when the dialog closes", async () => {
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		const importButton = screen.getByRole("button", { name: "Import Music" });
		importButton.focus();
		fireEvent.click(importButton);

		const dialog = screen.getByRole("dialog", { name: "Import Music" });
		await waitFor(() =>
			expect(dialog.contains(document.activeElement)).toBe(true),
		);
		fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Escape" });

		await waitFor(() =>
			expect(screen.queryByRole("dialog", { name: "Import Music" })).toBeNull(),
		);
		await waitFor(() => expect(document.activeElement).toBe(importButton));
	});

	it("disables Import Music when the Music Server lacks the Managed Import capability", async () => {
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "legacy",
			capabilities: ["api.v1", "future.unknown-feature.v2"],
		});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");

		const importButton = screen.getByRole("button", { name: "Import Music" });
		await waitFor(() =>
			expect(importButton.hasAttribute("disabled")).toBe(true),
		);
		expect(importButton.getAttribute("title")).toContain("Managed Import");
		fireEvent.click(importButton);
		expect(screen.queryByRole("heading", { name: "Import Music" })).toBeNull();
		expect(mocks.createManagedImportBatch).not.toHaveBeenCalled();
	});

	it("keeps Import Music available when the Music Server advertises extra unknown capabilities", async () => {
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "newer",
			capabilities: [
				"api.v1",
				"managed-import.v1",
				"future.unknown-feature.v2",
			],
		});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		await waitFor(() => expect(mocks.getHealth).toHaveBeenCalled());

		const importButton = screen.getByRole("button", { name: "Import Music" });
		expect(importButton.hasAttribute("disabled")).toBe(false);
		fireEvent.click(importButton);
		expect(screen.getByRole("heading", { name: "Import Music" })).toBeTruthy();
	});

	it("loads exact Artist tracks within the 200-track cap and reports loaded/total counts", async () => {
		mocks.listTracks.mockResolvedValue({ items: libraryTracks, total: 237 });
		renderWithQuery(<TracksPage artistId="guest-id" search="live" />);
		await screen.findByText("Anti-Hero");
		expect(mocks.listTracks).toHaveBeenLastCalledWith({
			limit: 200,
			artistId: "guest-id",
			q: "live",
		});
		expect(screen.getByText("Showing 2 of 237 tracks")).toBeTruthy();
	});

	it("clears filters even when an Artist has no matching Tracks", async () => {
		mocks.listTracks.mockResolvedValue({ items: [], total: 0 });
		const onClearFilters = vi.fn();
		renderWithQuery(
			<TracksPage artistId="unknown" onClearFilters={onClearFilters} />,
		);
		expect(
			await screen.findByText("No tracks match these filters."),
		).toBeTruthy();
		expect(screen.getByText("Showing 0 of 0 tracks")).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
		expect(onClearFilters).toHaveBeenCalledOnce();
	});

	it("uses URL Artist identity, separates cached filters, and clears the URL back to all Tracks", async () => {
		mocks.listTracks.mockImplementation(async (params) => ({
			items: [
				{
					...libraryTracks[0],
					title: params.artistId
						? `Tracks for ${params.artistId}`
						: "All tracks",
				},
			],
			total: 1,
		}));
		const root = createRootRoute();
		const tracksRoute = Route.update({
			getParentRoute: () => root,
			path: "/library/tracks/",
			id: "/library/tracks/",
		} as never);
		const router = createRouter({
			routeTree: root.addChildren([tracksRoute]),
			history: createMemoryHistory({
				initialEntries: ["/library/tracks?artistId=first&q=live"],
			}),
		});
		renderWithQuery(<RouterProvider router={router} />);
		await screen.findByText("Tracks for first");
		expect(mocks.listTracks).toHaveBeenLastCalledWith({
			limit: 200,
			artistId: "first",
			q: "live",
		});
		await act(async () =>
			router.navigate({
				to: "/library/tracks",
				search: { artistId: "second", q: "live" },
			}),
		);
		await screen.findByText("Tracks for second");
		expect(screen.queryByText("Tracks for first")).toBeNull();
		await act(async () =>
			router.navigate({
				to: "/library/tracks",
				search: { artistId: "first", q: "studio" },
			}),
		);
		await waitFor(() =>
			expect(mocks.listTracks).toHaveBeenLastCalledWith({
				limit: 200,
				artistId: "first",
				q: "studio",
			}),
		);
		fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
		await screen.findByText("All tracks");
		expect(router.state.location.search).toEqual({});
		expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
	});

	it("rejects structured Artist filters at the URL boundary before loading Tracks", async () => {
		const root = createRootRoute();
		const tracksRoute = Route.update({
			getParentRoute: () => root,
			path: "/library/tracks/",
			id: "/library/tracks/",
		} as never);
		const router = createRouter({
			routeTree: root.addChildren([tracksRoute]),
			history: createMemoryHistory({
				initialEntries: [
					"/library/tracks?artistId=%5B%22first%22%2C%22second%22%5D",
				],
			}),
			defaultErrorComponent: () => <p role="alert">Invalid track filters</p>,
		});
		renderWithQuery(<RouterProvider router={router} />);
		expect(await screen.findByRole("alert")).toHaveProperty(
			"textContent",
			"Invalid track filters",
		);
		expect(mocks.listTracks).not.toHaveBeenCalled();
	});

	it("renders the shared compact header and the track list", async () => {
		renderWithQuery(<TracksPage />);

		await screen.findByText("Anti-Hero");
		expect(screen.getByTestId("track-numbering").textContent).toBe("list");
		expect(screen.getByTestId("track-show-favorite").textContent).toBe("true");
		expect(screen.getByTestId("track-compact").textContent).toBe("true");
		expect(mocks.listTracks).toHaveBeenCalledTimes(1);
		expect(mocks.listTracks).toHaveBeenLastCalledWith({ limit: 200 });

		const header = screen
			.getByRole("heading", { name: "Tracks" })
			.closest("header");
		expect(header).toBeTruthy();
		expect(header?.className).toContain("sticky");
		expect(header?.className).toContain("top-0");
		expect(header?.className).toContain("py-3");
		expect(header?.querySelector(".page-content-column")).toBeTruthy();
		expect(screen.queryByPlaceholderText(/Search/)).toBeNull();
		expect(screen.getByTestId("tracks-page-shell").className).toContain(
			"overflow-hidden",
		);
		expect(screen.getByTestId("tracks-page-content").className).toContain(
			"[scrollbar-width:none]",
		);
		expect(screen.getByTestId("tracks-page-content").className).toContain(
			"py-6",
		);
	});

	it("imports supported folder audio without sending client paths", async () => {
		await openImportMusicDialog();
		const folderInput = screen.getByLabelText("Audio folder");
		expect(folderInput.getAttribute("webkitdirectory")).toBe("");
		const track = selectFolderWithIgnoredFiles();

		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledOnce(),
		);
		expect(mocks.createManagedImportJob).toHaveBeenCalledOnce();
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledWith(
			"import-1",
			"track.FLAC",
			track,
			expect.any(Function),
			expect.any(AbortSignal),
		);
	});

	it("uses native desktop selection and opaque streaming upload in shared dialog", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFolder.mockResolvedValue([
			{ id: "opaque-selection", name: "track.flac", size: 42 },
		]);
		mocks.desktopUploadImportFile.mockResolvedValue(
			new Response(JSON.stringify(createImportPreview("import-1")), {
				status: 200,
				headers: { "content-type": "application/json" },
			}),
		);

		await openImportMusicDialog();
		expect(screen.queryByLabelText("Audio folder")).toBeNull();
		fireEvent.click(
			screen.getByRole("button", { name: "Select audio folder" }),
		);

		await vi.waitFor(() =>
			expect(mocks.desktopUploadImportFile).toHaveBeenCalledWith(
				"opaque-selection",
				"import-1",
				expect.any(Function),
				expect.any(AbortSignal),
			),
		);
		expect(mocks.uploadManagedImportFile).not.toHaveBeenCalled();
		expect(await screen.findByText("import-1")).toBeTruthy();
		expect(
			screen.queryByRole("button", { name: "Select audio folder" }),
		).toBeNull();
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));
		await vi.waitFor(() =>
			expect(mocks.releaseDesktopImportSelections).toHaveBeenCalledWith([
				"opaque-selection",
			]),
		);
	});

	it("releases native selections when canceling an uncommitted import", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(true);
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockResolvedValue([
			{ id: "opaque-selection", name: "track.flac", size: 42 },
		]);
		mocks.desktopUploadImportFile.mockResolvedValue(
			new Response(JSON.stringify(createImportPreview("import-1")), {
				status: 200,
				headers: { "content-type": "application/json" },
			}),
		);

		await openImportMusicDialog();
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));
		await screen.findByText("import-1");
		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await vi.waitFor(() =>
			expect(mocks.releaseDesktopImportSelections).toHaveBeenCalledWith([
				"opaque-selection",
			]),
		);
		expect(mocks.cancelManagedImportBatch).toHaveBeenCalledWith("batch-1");
		await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
		confirmClose.mockRestore();
	});

	it("locks the dialog while native recursive selection is pending", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		let finishSelection: (files: never[]) => void = () => undefined;
		mocks.selectDesktopImportFolder.mockReturnValue(
			new Promise((resolve) => {
				finishSelection = resolve;
			}),
		);

		await openImportMusicDialog();
		fireEvent.click(
			screen.getByRole("button", { name: "Select audio folder" }),
		);

		await vi.waitFor(() =>
			expect(screen.getByRole("button", { name: "Cancel" })).toHaveProperty(
				"disabled",
				true,
			),
		);
		finishSelection([]);
		await vi.waitFor(() =>
			expect(screen.getByRole("button", { name: "Cancel" })).toHaveProperty(
				"disabled",
				false,
			),
		);
		expect(mocks.createManagedImportBatch).not.toHaveBeenCalled();
	});

	it("allows canceling after native selection advances to upload", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(true);
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockResolvedValue([
			{ id: "opaque-selection", name: "track.flac", size: 42 },
		]);
		let uploadSignal: AbortSignal | undefined;
		mocks.desktopUploadImportFile.mockImplementation(
			(_selectionId, _jobId, _onProgress, signal: AbortSignal) => {
				uploadSignal = signal;
				return new Promise((_resolve, reject) => {
					signal.addEventListener("abort", () =>
						reject(new DOMException("canceled", "AbortError")),
					);
				});
			},
		);

		await openImportMusicDialog();
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));
		await vi.waitFor(() => expect(uploadSignal).toBeInstanceOf(AbortSignal));
		expect(screen.getByRole("button", { name: "Cancel" })).toHaveProperty(
			"disabled",
			false,
		);
		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await vi.waitFor(() => expect(uploadSignal?.aborted).toBe(true));
		await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
		confirmClose.mockRestore();
	});

	it("shows structured native picker errors in the shared dialog", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockRejectedValue({
			code: "selection_unavailable",
			message: "Selected import file could not be read.",
		});

		await openImportMusicDialog();
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));

		expect(
			await screen.findByText("Selected import file could not be read."),
		).toBeTruthy();
		expect(mocks.createManagedImportBatch).not.toHaveBeenCalled();
	});

	it("renders native progress, structured rejections, duplicate decisions, and terminal results like Web", async () => {
		// Desktop parity for issue #55: the shared Import Music dialog drives
		// the native transport through the same states the Web journey shows.
		mocks.isDesktopClient.mockReturnValue(true);
		// Native uploads run one at a time, so server jobs are created in
		// selection order.
		mocks.createManagedImportJob.mockReset();
		for (const jobId of [
			"accepted-selection",
			"broken-selection",
			"remaster-selection",
		]) {
			mocks.createManagedImportJob.mockResolvedValueOnce({
				id: jobId,
				status: "uploading",
				revision: 1,
			});
		}
		mocks.selectDesktopImportFolder.mockResolvedValue([
			{ id: "accepted-selection", name: "track.flac", size: 42 },
			{ id: "broken-selection", name: "broken.flac", size: 7 },
			{ id: "remaster-selection", name: "remaster.opus", size: 84 },
		]);
		const acceptedPreview = {
			...createImportPreview("accepted-selection"),
			file: {
				...createImportPreview("accepted-selection").file,
				originalFilename: "track.flac",
			},
		};
		const remasterPreview = {
			...createImportPreview("remaster-selection"),
			file: {
				...createImportPreview("remaster-selection").file,
				originalFilename: "remaster.opus",
				title: "Remaster",
				artists: ["Test Artist"],
				album: "Strict Import Tests",
				format: "opus",
			},
			duplicateClassification: "none",
			matchingTracks: [
				{
					trackId: "existing-track",
					title: "Remaster",
					artists: ["Test Artist"],
					album: "Strict Import Tests",
					discNo: 1,
					trackNo: 3,
					format: "ogg",
					durationMs: 245,
				},
			],
		};
		let releaseAcceptedUpload: (() => void) | undefined;
		mocks.desktopUploadImportFile.mockImplementation(
			(
				selectionId: string,
				_jobId: string,
				onProgress: (p: number) => void,
			) => {
				if (selectionId === "accepted-selection") {
					onProgress(45);
					return new Promise((resolve) => {
						releaseAcceptedUpload = () =>
							resolve(
								new Response(JSON.stringify(acceptedPreview), {
									status: 200,
									headers: { "content-type": "application/json" },
								}),
							);
					});
				}
				if (selectionId === "broken-selection") {
					return Promise.resolve(
						new Response(
							JSON.stringify({
								code: "invalid_metadata",
								message: "TITLE is required",
							}),
							{ status: 422, headers: { "content-type": "application/json" } },
						),
					);
				}
				return Promise.resolve(
					new Response(JSON.stringify(remasterPreview), {
						status: 200,
						headers: { "content-type": "application/json" },
					}),
				);
			},
		);
		mocks.getManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 3,
			files: [
				createBatchFile("accepted-selection", true),
				{
					jobId: "broken-selection",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					originalFilename: "broken.flac",
					selected: false,
					errorCode: "invalid_metadata",
					errorReason: "TITLE is required",
				},
				{
					...createBatchFile("remaster-selection", true),
					selected: true,
					preview: remasterPreview,
				},
			],
		});
		mocks.confirmManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "completed",
			revision: 5,
			files: [
				{
					jobId: "accepted-selection",
					state: "completed",
					status: "committed",
					revision: 3,
					validationProgress: 100,
					selected: true,
					outcome: "imported",
					trackId: "imported-track",
				},
				{
					jobId: "broken-selection",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					selected: false,
					outcome: "rejected",
					errorCode: "invalid_metadata",
					errorReason: "TITLE is required",
				},
				{
					jobId: "remaster-selection",
					state: "completed",
					status: "committed",
					revision: 3,
					validationProgress: 100,
					selected: true,
					outcome: "imported",
					trackId: "remaster-track",
				},
			],
		});

		await openImportMusicDialog();
		fireEvent.click(
			screen.getByRole("button", { name: "Select audio folder" }),
		);

		// Native progress drives the same progress bar the Web upload uses.
		const progressbar = await screen.findByRole("progressbar", {
			name: "track.flac upload progress",
		});
		await vi.waitFor(() =>
			expect(progressbar.getAttribute("aria-valuenow")).toBe("45"),
		);
		expect(mocks.uploadManagedImportFile).not.toHaveBeenCalled();
		// A second slot processes siblings while the first transfer remains active.
		expect(mocks.desktopUploadImportFile).toHaveBeenCalledTimes(3);
		releaseAcceptedUpload?.();

		// Structured server rejections render as Web does.
		expect(await screen.findByText("TITLE is required")).toBeTruthy();
		// The review step scrolls as one region; a nested scroll container with
		// overscroll-contain would swallow mouse-wheel scrolling.
		const preview = screen.getByRole("region", { name: "Import Preview" });
		expect(preview.className).not.toContain("overflow-y-auto");
		expect(preview.className).not.toContain("overscroll-contain");
		expect(preview.parentElement?.className).toContain("overflow-y-auto");
		expect(preview.parentElement?.className).toContain("overscroll-contain");
		expect(preview.parentElement?.className).toContain(
			"[scrollbar-width:none]",
		);
		expect(
			screen.getByRole("checkbox", { name: "Select broken.flac" }),
		).toHaveProperty("disabled", true);
		expect(
			screen.getByRole("checkbox", { name: "Select track.flac" }),
		).toHaveProperty("checked", true);

		// A Possible Duplicate requires an explicit decision before confirming.
		await screen.findByText("Track Replacement");
		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", true);
		fireEvent.click(
			screen.getByRole("radio", { name: "Replace existing Track" }),
		);
		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", false);
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));

		await vi.waitFor(() =>
			expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
				"batch-1",
				3,
				["accepted-selection", "remaster-selection"],
				[
					{
						jobId: "remaster-selection",
						action: "replace_existing",
						trackId: "existing-track",
					},
				],
				undefined,
			),
		);
		expect(await screen.findAllByText("Imported")).toHaveLength(2);
		expect(screen.getByText("Rejected")).toBeTruthy();
		await vi.waitFor(() =>
			expect(mocks.releaseDesktopImportSelections).toHaveBeenCalledWith([
				"accepted-selection",
				"broken-selection",
				"remaster-selection",
			]),
		);
	});

	it("limits recursive folder uploads to two concurrent files", async () => {
		mockClientFileJobs();
		const uploads = mockDeferredUploads();
		await openImportMusicDialog();
		selectAudioFolder(
			["one", "two", "three", "four"].map(
				(name) => new File([name], `${name}.flac`),
			),
		);

		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(2),
		);
		expect(uploads.getMaximumActiveUploads()).toBe(2);
		uploads.releases.shift()?.();
		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(3),
		);
		uploads.releases.shift()?.();
		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(4),
		);
		for (const release of uploads.releases) release();
		await vi.waitFor(() => expect(uploads.getActiveUploads()).toBe(0));
	});

	it("retries only an interrupted folder file in its active job", async () => {
		mockRetryableBatchResponses();
		const finishRetry = mockInterruptedFolderUpload();
		await openImportMusicDialog();
		vi.useFakeTimers();
		await act(async () =>
			selectAudioFolder([
				new File(["one"], "one.flac"),
				new File(["two"], "two.flac"),
			]),
		);
		await act(async () => {
			await vi.advanceTimersByTimeAsync(2000);
		});

		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledTimes(3),
		);
		const interruptedRow = screen.getByText("one.flac").closest("article");
		expect(interruptedRow?.textContent).toContain("Uploading");
		expect(
			interruptedRow
				?.querySelector('[role="progressbar"]')
				?.getAttribute("aria-valuenow"),
		).toBe("45");
		const details = screen.getByRole("button", {
			name: "Details for one.flac",
		});
		fireEvent.click(details);
		finishRetry();
		await vi.waitFor(() =>
			expect(interruptedRow?.textContent).toContain("Accepted"),
		);
		expect(details.isConnected).toBe(true);
		expect(details.getAttribute("aria-expanded")).toBe("true");
		expectUploadAttempts("import-1", 2);
		expectUploadAttempts("import-2", 1);
		expect(mocks.uploadManagedImportFile.mock.calls[2]?.[4]).toBeInstanceOf(
			AbortSignal,
		);
	});

	it("groups album metadata and reveals per-file details only on request", async () => {
		mocks.uploadManagedImportFile.mockImplementation(async (jobId: string) => ({
			...createImportPreview(jobId),
		}));
		mocks.getManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 5,
			files: [
				createBatchFile("import-1", true),
				createBatchFile("import-2", true),
			],
		});
		await openImportMusicDialog();
		selectAudioFolder([
			new File(["one"], "one.flac"),
			new File(["two"], "two.flac"),
		]);
		await screen.findByText("2 of 2 ready");
		expect(screen.getAllByText("Strict Import Tests")).toHaveLength(1);
		expect(screen.getAllByText("Test Artist")).toHaveLength(2);
		expect(screen.getAllByText("import-1")).toHaveLength(1);
		expect(
			screen.queryByText("Identification is off", { exact: false }),
		).toBeNull();
		const details = screen.getByRole("button", {
			name: "Details for import-1.flac",
		});
		fireEvent.click(details);
		expect(details.getAttribute("aria-expanded")).toBe("true");
		expect(
			screen.queryByText("Identification is off", { exact: false }),
		).toBeNull();
		fireEvent.click(details);
		expect(screen.queryByText("File tags · Identification is off")).toBeNull();
	});

	it("imports a selected file and reports a rejected sibling independently", async () => {
		mocks.listTracks
			.mockResolvedValueOnce({ items: libraryTracks })
			.mockResolvedValue({
				items: [
					...libraryTracks,
					{
						id: "imported-track",
						title: "Inspection Fixture",
						artistName: "Test Artist",
						albumId: "imported-album",
						durationMs: 250,
						format: "flac",
					},
				],
			});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");

		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		expect(screen.getByRole("dialog")).toBeTruthy();
		expect(screen.getByRole("heading", { name: "Import Music" })).toBeTruthy();
		const fileInput = screen.getByLabelText("Audio files");
		expect(fileInput.getAttribute("accept")).toBe(
			".flac,.mp3,.m4a,.ogg,.opus,.wav",
		);
		const acceptedFile = new File(["flac bytes"], "strict-import.flac", {
			type: "audio/flac",
		});
		const rejectedFile = new File(["broken"], "broken.flac", {
			type: "audio/flac",
		});
		mocks.uploadManagedImportFile
			.mockImplementationOnce(async (_jobId, _filename, _file, onProgress) => {
				onProgress?.(60);
				return {
					jobId: "import-1",
					status: "awaiting_confirmation",
					revision: 2,
					file: {
						originalFilename: "strict-import.flac",
						title: "Inspection Fixture",
						artists: ["Test Artist"],
						albumArtists: ["Test Album Artist"],
						album: "Strict Import Tests",
						genres: ["Electronic"],
						trackNo: 3,
						discNo: 1,
						durationMs: 250,
						format: "flac",
						artworkMediaType: "image/png",
					},
				};
			})
			.mockRejectedValueOnce(
				new ApiError(422, {
					error: "invalid_metadata",
					code: "invalid_metadata",
					message: "TITLE is required",
				}),
			);
		fireEvent.change(fileInput, {
			target: { files: [acceptedFile, rejectedFile] },
		});

		await screen.findByText("Inspection Fixture");
		expect(screen.getByText("Test Artist")).toBeTruthy();
		expect(screen.getByText("Strict Import Tests")).toBeTruthy();
		expect(mocks.createManagedImportBatch).toHaveBeenCalledOnce();
		expect(mocks.createManagedImportJob).toHaveBeenCalledTimes(2);
		expect(mocks.createManagedImportJob).toHaveBeenNthCalledWith(
			1,
			"batch-1",
			expect.any(String),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledWith(
			"import-1",
			"strict-import.flac",
			acceptedFile,
			expect.any(Function),
			expect.any(AbortSignal),
		);
		expect(screen.getByText("Rejected")).toBeTruthy();
		expect(screen.getByText("TITLE is required")).toBeTruthy();
		const acceptedCheckbox = screen.getByRole("checkbox", {
			name: "Select strict-import.flac",
		});
		expect(acceptedCheckbox).not.toHaveProperty("disabled", true);
		fireEvent.click(acceptedCheckbox);
		expect(acceptedCheckbox).toHaveProperty("checked", false);
		fireEvent.click(acceptedCheckbox);
		expect(acceptedCheckbox).toHaveProperty("checked", true);
		expect(
			screen.getByRole("checkbox", { name: "Select broken.flac" }),
		).toHaveProperty("disabled", true);

		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));

		await screen.findByText("Inspection Fixture");
		expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
			"batch-1",
			3,
			["import-1"],
			undefined,
			undefined,
		);
		await vi.waitFor(() => expect(mocks.listTracks).toHaveBeenCalledTimes(2));
		expect(screen.getByText("Imported")).toBeTruthy();
		expect(screen.getByRole("dialog")).toBeTruthy();
	});

	it("shows Exact Duplicate details without navigating away", async () => {
		mocks.uploadManagedImportFile.mockResolvedValueOnce({
			jobId: "import-1",
			status: "failed",
			revision: 2,
			duplicateClassification: "exact_duplicate",
			duplicateCandidates: [
				{
					trackId: "existing-track",
					title: "Inspection Fixture",
					artists: ["Test Artist"],
					album: "Strict Import Tests",
					discNo: 1,
					trackNo: 3,
					format: "flac",
					durationMs: 250,
				},
			],
			file: {
				originalFilename: "duplicate.flac",
				title: "Inspection Fixture",
				artists: ["Test Artist"],
				albumArtists: ["Test Album Artist"],
				album: "Strict Import Tests",
				genres: ["Electronic"],
				trackNo: 3,
				discNo: 1,
				durationMs: 250,
				format: "flac",
				artworkMediaType: "image/png",
			},
		});
		mocks.getManagedImportBatch.mockResolvedValueOnce({
			id: "batch-1",
			status: "uploading",
			revision: 2,
			files: [],
		});
		await openImportMusicDialog();
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: { files: [new File(["same"], "duplicate.flac")] },
		});

		const disclosure = await screen.findByText("View existing Track");
		expect(disclosure.closest("a")).toBeNull();
		fireEvent.click(disclosure);
		expect(disclosure.closest("details")?.open).toBe(true);
		expect(screen.queryByRole("radio")).toBeNull();
	});

	it("requires an explicit Possible Duplicate decision", async () => {
		const preview = {
			...(await mocks.uploadManagedImportFile.getMockImplementation()?.(
				"import-1",
			)),
			duplicateClassification: "none",
			matchingTracks: [
				{
					trackId: "existing-track",
					title: "Inspection Fixture",
					artists: ["Test Artist"],
					album: "Strict Import Tests",
					discNo: 1,
					trackNo: 3,
					format: "mp3",
					durationMs: 245,
				},
			],
		};
		mocks.uploadManagedImportFile.mockResolvedValueOnce(preview);
		mocks.getManagedImportBatch.mockResolvedValueOnce({
			id: "batch-1",
			status: "uploading",
			revision: 2,
			files: [
				{
					jobId: "import-1",
					state: "accepted",
					status: "awaiting_confirmation",
					revision: 2,
					validationProgress: 100,
					selected: false,
					preview,
				},
			],
		});
		await openImportMusicDialog();
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: { files: [new File(["different"], "candidate.flac")] },
		});

		await screen.findByText("Track Replacement");
		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", true);
		fireEvent.click(
			screen.getByRole("radio", { name: "Replace existing Track" }),
		);
		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", false);
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));
		await vi.waitFor(() =>
			expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
				"batch-1",
				3,
				["import-1"],
				[
					{
						jobId: "import-1",
						action: "replace_existing",
						trackId: "existing-track",
					},
				],
				undefined,
			),
		);
		expect(
			screen.getByRole("radio", { name: "Replace existing Track" }),
		).toBeTruthy();
		expect(screen.getByRole("radio", { name: "Do not import" })).toBeTruthy();
	});

	it("stops confirmation when refresh finds a Possible Duplicate", async () => {
		const preview = {
			...(await mocks.uploadManagedImportFile.getMockImplementation()?.(
				"import-1",
			)),
			duplicateClassification: "none",
			matchingTracks: [
				{
					trackId: "late-track",
					title: "Inspection Fixture",
					artists: ["Test Artist"],
					album: "Strict Import Tests",
					discNo: 1,
					trackNo: 3,
					format: "flac",
					durationMs: 250,
				},
			],
		};
		mocks.getManagedImportBatch
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			})
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 4,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 3,
						validationProgress: 100,
						selected: false,
						preview,
					},
				],
			});
		await openImportMusicDialog();
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: { files: [new File(["different"], "candidate.flac")] },
		});
		await screen.findByText("Inspection Fixture");
		const confirmButton = screen.getByRole("button", {
			name: "Confirm Import",
		});
		await vi.waitFor(() =>
			expect(confirmButton).toHaveProperty("disabled", false),
		);
		fireEvent.click(confirmButton);
		await vi.waitFor(() =>
			expect(mocks.getManagedImportBatch).toHaveBeenCalledTimes(2),
		);
		expect(mocks.confirmManagedImportBatch).not.toHaveBeenCalled();
		await screen.findByText("Track Replacement");
		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", true);
	});

	it("retains the created batch when the preview refresh fails", async () => {
		mocks.getManagedImportBatch
			.mockRejectedValueOnce(new Error("preview refresh unavailable"))
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			});
		mocks.confirmManagedImportBatch.mockResolvedValueOnce({
			id: "batch-1",
			status: "completed",
			revision: 5,
			files: [
				{
					jobId: "import-1",
					state: "completed",
					status: "committed",
					revision: 3,
					validationProgress: 100,
					selected: true,
					outcome: "imported",
					trackId: "imported-track",
				},
			],
		});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		const fileInput = screen.getByLabelText("Audio files");
		expect(fileInput.getAttribute("accept")).toBe(
			".flac,.mp3,.m4a,.ogg,.opus,.wav",
		);
		fireEvent.change(fileInput, {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});

		await waitFor(
			() =>
				expect(
					screen.getByRole("button", { name: "Confirm Import" }),
				).toHaveProperty("disabled", false),
			{ timeout: 4000 },
		);
		const confirmButton = screen.getByRole("button", {
			name: "Confirm Import",
		});
		expect(confirmButton).not.toHaveProperty("disabled", true);
		fireEvent.click(confirmButton);

		await screen.findByText("Imported");
		expect(mocks.getManagedImportBatch).toHaveBeenCalledTimes(2);
		expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
			"batch-1",
			3,
			["import-1"],
			undefined,
			undefined,
		);
	});

	it("shows every validation issue on a separate readable line", async () => {
		mocks.uploadManagedImportFile.mockRejectedValueOnce(
			new ApiError(422, {
				error: "invalid_metadata",
				code: "invalid_metadata",
				message: "Validation failed",
				issues: [
					{
						code: "invalid_metadata",
						field: "GENRE",
						reason: "required tag is missing",
					},
					{
						code: "missing_artwork",
						field: "artwork",
						reason: "embedded front cover is required",
					},
				],
			}),
		);
		mocks.getManagedImportBatch.mockRejectedValueOnce(
			new Error("Refresh unavailable"),
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [new File(["audio"], "broken.flac", { type: "audio/flac" })],
			},
		});
		const genre = await screen.findByText("Genre: required tag is missing");
		const artwork = screen.getByText(
			"Embedded front cover not found. Add an image marked as Front Cover.",
		);
		expect(genre.tagName).toBe("LI");
		expect(artwork.tagName).toBe("LI");
		expect(screen.queryByText("missing_artwork")).toBeNull();
	});

	it("does not retry an unavailable Desktop file selection", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockResolvedValue([
			{ id: "selection", name: "missing.flac", size: 42 },
			{ id: "ready-selection", name: "ready.flac", size: 42 },
		]);
		mocks.desktopUploadImportFile.mockRejectedValueOnce({
			code: "selection_unavailable",
			message: "Selected file is no longer available",
		});
		mocks.desktopUploadImportFile.mockResolvedValue(
			new Response(JSON.stringify(createImportPreview("import-2")), {
				status: 200,
			}),
		);
		let hasCanceledJob = false;
		mocks.cancelManagedImport.mockImplementation(async () => {
			hasCanceledJob = true;
		});

		mocks.getManagedImportBatch.mockImplementation(async () => ({
			id: "batch-1",
			status: "uploading",
			revision: hasCanceledJob ? 3 : 2,
			files: [
				...(hasCanceledJob
					? []
					: [{ ...createBatchFile("import-1", false), phase: "queued" }]),
				createBatchFile("import-2", true),
			],
		}));
		await openImportMusicDialog();
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));
		await screen.findByText("Selected file is no longer available");
		expect(mocks.desktopUploadImportFile).toHaveBeenCalledTimes(2);
		expect(
			screen.queryByRole("button", { name: "Retry failed uploads" }),
		).toBeNull();
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));
		await vi.waitFor(() =>
			expect(mocks.confirmManagedImportBatch).toHaveBeenCalled(),
		);
		expect(mocks.cancelManagedImport).toHaveBeenCalledWith("import-1");
	});

	it("preserves native validation issues after batch refresh", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockResolvedValue([
			{ id: "selection", name: "broken.flac", size: 42 },
		]);
		const issues = [
			{
				code: "invalid_metadata",
				field: "GENRE",
				reason: "required tag is missing",
			},
			{
				code: "missing_artwork",
				field: "artwork",
				reason: "embedded front cover is required",
			},
		];
		mocks.desktopUploadImportFile.mockResolvedValue(
			new Response(
				JSON.stringify({
					error: "invalid_metadata",
					code: "invalid_metadata",
					message: "Validation failed",
					issues,
				}),
				{ status: 422 },
			),
		);
		mocks.getManagedImportBatch.mockResolvedValue({
			id: "batch-1",
			status: "uploading",
			revision: 2,
			files: [
				{
					jobId: "import-1",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					selected: false,
					errorCode: "invalid_metadata",
					errorReason: "required tag is missing",
					issues,
				},
			],
		});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));
		await waitFor(() => expect(mocks.getManagedImportBatch).toHaveBeenCalled());
		expect(
			(await screen.findByText("Genre: required tag is missing")).tagName,
		).toBe("LI");
		expect(
			screen.getByText(
				"Embedded front cover not found. Add an image marked as Front Cover.",
			).tagName,
		).toBe("LI");
	});

	it("closes a rejected-only import when the batch is already gone", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(true);
		mocks.uploadManagedImportFile.mockRejectedValueOnce(
			new ApiError(422, {
				error: "audio_decode_failed",
				code: "audio_decode_failed",
				message: "audio stream failed full decode",
			}),
		);
		mocks.getManagedImportBatch.mockResolvedValueOnce({
			id: "batch-1",
			status: "uploading",
			revision: 2,
			files: [
				{
					jobId: "import-1",
					state: "rejected",
					status: "failed",
					revision: 1,
					validationProgress: 100,
					originalFilename: "broken.flac",
					selected: false,
					errorCode: "audio_decode_failed",
					errorReason: "audio stream failed full decode",
				},
			],
		});
		mocks.cancelManagedImportBatch.mockRejectedValueOnce(
			new ApiError(404, {
				error: "not_found",
				code: "import_not_found",
				message: "Managed Import Job not found",
			}),
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [new File(["broken"], "broken.flac", { type: "audio/flac" })],
			},
		});
		await screen.findByText("audio stream failed full decode");

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
		expect(screen.queryByText("Managed Import Job not found")).toBeNull();
		expect(mocks.cancelManagedImportBatch).toHaveBeenCalledWith("batch-1");
		expect(confirmClose).not.toHaveBeenCalled();
		confirmClose.mockRestore();
	});

	it("confirms modal close and cancels uncommitted server staging", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(true);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		await screen.findByText("Inspection Fixture");

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await vi.waitFor(() =>
			expect(mocks.cancelManagedImportBatch).toHaveBeenCalledWith("batch-1"),
		);
		expect(confirmClose).toHaveBeenCalledOnce();
		await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
		confirmClose.mockRestore();
	});

	it("aborts active uploads before canceling their server batch", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(true);
		let uploadSignal: AbortSignal | undefined;
		mocks.uploadManagedImportFile.mockImplementationOnce(
			(_jobId, _filename, _file, _onProgress, signal: AbortSignal) => {
				uploadSignal = signal;
				return new Promise((_resolve, reject) => {
					signal.addEventListener("abort", () =>
						reject(new DOMException("canceled", "AbortError")),
					);
				});
			},
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "active.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledOnce(),
		);
		expect(screen.getByLabelText("Audio files")).toHaveProperty(
			"disabled",
			true,
		);
		expect(
			screen.getByRole("button", { name: "Close Import Music" }),
		).toHaveProperty("disabled", false);

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await vi.waitFor(() => expect(uploadSignal?.aborted).toBe(true));
		expect(mocks.cancelManagedImportBatch).toHaveBeenCalledWith("batch-1");
		await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
		confirmClose.mockRestore();
	});

	it("keeps uncommitted import open when modal close is not confirmed", async () => {
		const confirmClose = vi.spyOn(window, "confirm").mockReturnValue(false);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		await screen.findByText("Inspection Fixture");

		fireEvent.click(screen.getByRole("button", { name: "Close Import Music" }));

		expect(confirmClose).toHaveBeenCalledOnce();
		expect(mocks.cancelManagedImportBatch).not.toHaveBeenCalled();
		expect(screen.getByRole("dialog")).toBeTruthy();
		confirmClose.mockRestore();
	});

	it("reconciles an accepted upload whose response was lost", async () => {
		mocks.uploadManagedImportFile.mockRejectedValueOnce(
			new Error("upload response lost"),
		);
		mocks.getManagedImportBatch
			.mockRejectedValueOnce(new Error("preview refresh unavailable"))
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		await waitFor(
			() =>
				expect(
					screen.getByRole("button", { name: "Confirm Import" }),
				).toHaveProperty("disabled", false),
			{ timeout: 4000 },
		);

		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));

		await vi.waitFor(() =>
			expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
				"batch-1",
				3,
				["import-1"],
				undefined,
				undefined,
			),
		);
		expect(screen.queryByText("upload response lost")).toBeNull();
	});

	it("retries an unresolved server job whose create response was lost", async () => {
		let clientFileId = "";
		mocks.createManagedImportJob
			.mockReset()
			.mockImplementationOnce((_batchId, currentClientFileId) => {
				clientFileId = currentClientFileId;
				return Promise.reject(new Error("job response lost"));
			});
		mocks.getManagedImportBatch
			.mockImplementationOnce(async () => ({
				id: "batch-1",
				status: "uploading",
				revision: 2,
				files: [
					{
						jobId: "server-import-1",
						clientFileId,
						state: "unresolved",
						status: "uploading",
						revision: 1,
						validationProgress: 0,
						selected: false,
					},
				],
			}))
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [
					{
						jobId: "server-import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		const file = new File(["flac bytes"], "strict-import.flac", {
			type: "audio/flac",
		});
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: { files: [file] },
		});

		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalledWith(
				"server-import-1",
				"strict-import.flac",
				file,
				expect.any(Function),
				expect.any(AbortSignal),
			),
		);
		expect(await screen.findByText("Accepted")).toBeDefined();
		expect(screen.queryByText("job response lost")).toBeNull();
	});

	it("correlates a recovered job after an earlier create genuinely fails", async () => {
		let recoveredClientFileId = "";
		mocks.createManagedImportJob
			.mockReset()
			.mockRejectedValueOnce(new Error("first create failed"))
			.mockImplementationOnce((_batchId, clientFileId) => {
				recoveredClientFileId = clientFileId;
				return Promise.reject(new Error("second response lost"));
			});
		mocks.getManagedImportBatch
			.mockImplementationOnce(async () => ({
				id: "batch-1",
				status: "uploading" as const,
				revision: 2,
				files: [
					{
						jobId: "server-import-2",
						clientFileId: recoveredClientFileId,
						state: "unresolved" as const,
						status: "uploading" as const,
						revision: 1,
						validationProgress: 0,
						selected: false,
					},
				],
			}))
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [],
			});
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		const firstFile = new File(["first"], "first.flac", {
			type: "audio/flac",
		});
		const secondFile = new File(["second"], "second.flac", {
			type: "audio/flac",
		});
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: { files: [firstFile, secondFile] },
		});

		await vi.waitFor(() =>
			expect(mocks.uploadManagedImportFile).toHaveBeenCalled(),
		);
		expect(mocks.uploadManagedImportFile).toHaveBeenCalledWith(
			"server-import-2",
			"second.flac",
			secondFile,
			expect.any(Function),
			expect.any(AbortSignal),
		);
		expect(mocks.uploadManagedImportFile).not.toHaveBeenCalledWith(
			"server-import-2",
			"first.flac",
			firstFile,
			expect.any(Function),
			expect.any(AbortSignal),
		);
	});

	it("preserves an explicit deselection across confirmation refresh", async () => {
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		const checkbox = await screen.findByRole("checkbox", {
			name: "Select strict-import.flac",
		});
		fireEvent.click(checkbox);
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));

		expect(
			screen.getByRole("button", { name: "Confirm Import" }),
		).toHaveProperty("disabled", true);
		expect(mocks.confirmManagedImportBatch).not.toHaveBeenCalled();
	});

	it("freezes file selection while confirmation is pending", async () => {
		let finishConfirmation: (() => void) | undefined;
		mocks.confirmManagedImportBatch.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					finishConfirmation = () =>
						resolve({
							id: "batch-1",
							status: "completed",
							revision: 4,
							files: [],
						});
				}),
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		const checkbox = await screen.findByRole("checkbox", {
			name: "Select strict-import.flac",
		});
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));
		await vi.waitFor(() => expect(checkbox).toHaveProperty("disabled", true));
		finishConfirmation?.();
	});

	it("keeps selection frozen when the server batch remains confirming", async () => {
		mocks.getManagedImportBatch
			.mockResolvedValueOnce({
				id: "batch-1",
				status: "uploading",
				revision: 3,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			})
			.mockResolvedValue({
				id: "batch-1",
				status: "confirming",
				revision: 4,
				files: [
					{
						jobId: "import-1",
						state: "accepted",
						status: "awaiting_confirmation",
						revision: 2,
						validationProgress: 100,
						selected: true,
					},
				],
			});
		mocks.confirmManagedImportBatch.mockRejectedValueOnce(
			new Error("confirmation response lost"),
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.change(screen.getByLabelText("Audio files"), {
			target: {
				files: [
					new File(["flac bytes"], "strict-import.flac", {
						type: "audio/flac",
					}),
				],
			},
		});
		const checkbox = await screen.findByRole("checkbox", {
			name: "Select strict-import.flac",
		});
		fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));
		await screen.findByText("confirmation response lost");
		expect(checkbox).toHaveProperty("disabled", true);
		expect(
			screen.getByRole("button", { name: "Close Import Music" }),
		).toHaveProperty("disabled", false);
		expect(screen.getByRole("button", { name: "Cancel" })).toHaveProperty(
			"disabled",
			true,
		);
	});

	it("uploads at most two desktop files at a time", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.selectDesktopImportFiles.mockResolvedValue([
			{ id: "selection-1", name: "one.flac", size: 3 },
			{ id: "selection-2", name: "two.flac", size: 3 },
			{ id: "selection-3", name: "three.flac", size: 5 },
		]);
		mocks.createManagedImportJob
			.mockReset()
			.mockResolvedValueOnce({
				id: "import-1",
				status: "uploading",
				revision: 1,
			})
			.mockResolvedValueOnce({
				id: "import-2",
				status: "uploading",
				revision: 1,
			})
			.mockResolvedValueOnce({
				id: "import-3",
				status: "uploading",
				revision: 1,
			});
		const releases: Array<() => void> = [];
		let activeUploads = 0;
		let maximumActiveUploads = 0;
		mocks.desktopUploadImportFile.mockImplementation(
			async (_selectionId, jobId) => {
				activeUploads++;
				maximumActiveUploads = Math.max(maximumActiveUploads, activeUploads);
				await new Promise<void>((resolve) => releases.push(resolve));
				activeUploads--;
				return new Response(JSON.stringify(createImportPreview(jobId)), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			},
		);
		renderWithQuery(<TracksPage />);
		await screen.findByText("Anti-Hero");
		fireEvent.click(screen.getByRole("button", { name: "Import Music" }));
		fireEvent.click(screen.getByRole("button", { name: "Select audio files" }));

		await vi.waitFor(() =>
			expect(mocks.desktopUploadImportFile).toHaveBeenCalledTimes(2),
		);
		releases.shift()?.();
		await vi.waitFor(() =>
			expect(mocks.desktopUploadImportFile).toHaveBeenCalledTimes(3),
		);
		for (const release of releases) release();
		await vi.waitFor(() => expect(activeUploads).toBe(0));
		expect(maximumActiveUploads).toBe(2);
	});
});
