import type {
	ManagedImportPreview,
	Track,
	TrackReplacementPreview,
} from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useTrackReplacementFlow } from "./use-track-replacement-flow";

const createTrackReplacement = vi.fn();
const uploadManagedImportFile = vi.fn();
const confirmTrackReplacement = vi.fn();
const cancelManagedImport = vi.fn();
const stopPlayback = vi.fn();
const refreshQueue = vi.fn();
let currentTrackId: string | null = null;

vi.mock("#/lib/api", () => ({
	apiClient: {
		createTrackReplacement: (...args: unknown[]) =>
			createTrackReplacement(...args),
		uploadManagedImportFile: (...args: unknown[]) =>
			uploadManagedImportFile(...args),
		confirmTrackReplacement: (...args: unknown[]) =>
			confirmTrackReplacement(...args),
		cancelManagedImport: (...args: unknown[]) => cancelManagedImport(...args),
	},
}));

vi.mock("@repo/ui", () => ({
	usePlayback: () => ({
		currentTrack: currentTrackId ? { id: currentTrackId } : null,
		stopPlayback,
		refreshQueue,
	}),
}));

const selectDesktopImportFiles = vi.fn();
const releaseDesktopImportSelections = vi.fn();
const desktopUploadImportFile = vi.fn();

vi.mock("#/desktop/bridge", () => ({
	isDesktopClient: () => false,
	selectDesktopImportFiles: (...args: unknown[]) =>
		selectDesktopImportFiles(...args),
	releaseDesktopImportSelections: (...args: unknown[]) =>
		releaseDesktopImportSelections(...args),
	desktopUploadImportFile: (...args: unknown[]) =>
		desktopUploadImportFile(...args),
}));

const track = {
	id: "t1",
	title: "Welcome to New York",
	artistName: "Taylor Swift",
	albumId: "a1",
	albumTitle: "1989",
	discNo: 1,
	durationMs: 212_000,
	format: "flac",
	artists: [],
	genres: [],
} as unknown as Track;

const replacement: TrackReplacementPreview = {
	trackId: "t1",
	trackTitle: "Welcome to New York",
	sourceFormat: {
		field: "format",
		current: "flac",
		replacement: "flac",
		isChanged: false,
	},
	technicalProperties: [],
	metadata: [
		{
			field: "genres",
			current: "Pop",
			replacement: "Synthpop",
			isChanged: true,
		},
	],
	library: {
		currentAlbumId: "a1",
		movesAlbum: false,
		createsAlbum: false,
		removesEmptyAlbum: false,
		removesEmptyArtists: [],
		createsArtists: [],
		createsGenres: ["Synthpop"],
	},
	artwork: {
		currentMediaType: "image/png",
		currentSha256: "a",
		replacementMediaType: "image/png",
		replacementSha256: "a",
		isChanged: false,
		replacesAlbumArtwork: false,
	},
	canonicalPath: {
		field: "canonicalPath",
		current: "library/x/y/01-01-welcome-t1.flac",
		replacement: "library/x/y/01-01-welcome-t1.flac",
		isChanged: false,
	},
	oldFile: { path: "library/x/y/01-01-welcome-t1.flac", sizeBytes: 10 },
	playlistReferences: [],
	queueReferences: [{ userId: "user-1", itemCount: 1 }],
	confirmationToken: "token-1",
};

function awaitingPreview(): ManagedImportPreview {
	return {
		jobId: "job-1",
		status: "awaiting_confirmation",
		revision: 2,
		duplicateClassification: "none",
		replacement,
		file: {
			originalFilename: "better.flac",
			title: "Welcome to New York",
			artists: ["Taylor Swift"],
			albumArtists: ["Taylor Swift"],
			album: "1989",
			genres: ["Synthpop"],
			trackNo: 1,
			discNo: 1,
			durationMs: 212_000,
			sampleRateHz: 44_100,
			channelCount: 2,
			bitrateKbps: 900,
			artworkMediaType: "image/png",
			format: "flac",
			container: "flac",
			codec: "flac",
			bitDepth: 16,
		},
	};
}

function wrapper({ children }: { children: ReactNode }) {
	const queryClient = new QueryClient();
	return (
		<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
	);
}

describe("useTrackReplacementFlow", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		currentTrackId = null;
		createTrackReplacement.mockResolvedValue({
			id: "job-1",
			status: "uploading",
			revision: 1,
			validationProgress: 0,
			replacesTrackId: "t1",
		});
		uploadManagedImportFile.mockResolvedValue(awaitingPreview());
		confirmTrackReplacement.mockResolvedValue({
			jobId: "job-1",
			status: "committed",
			revision: 3,
			trackId: "t1",
			deletedFiles: 1,
		});
		cancelManagedImport.mockResolvedValue(undefined);
		refreshQueue.mockResolvedValue(undefined);
		releaseDesktopImportSelections.mockResolvedValue(undefined);
	});

	it("keeps a committed replacement completed when cache refresh fails", async () => {
		refreshQueue.mockRejectedValue(new Error("offline"));
		const onReplaced = vi.fn();
		const queryClient = new QueryClient({
			defaultOptions: { queries: { retry: false } },
		});
		queryClient.setQueryData(["library", "search", "old credit"], {
			tracks: [],
		});
		const { result } = renderHook(() => useTrackReplacementFlow(onReplaced), {
			wrapper: ({ children }) => (
				<QueryClientProvider client={queryClient}>
					{children}
				</QueryClientProvider>
			),
		});

		act(() => result.current.open(track));
		await act(() =>
			result.current.replaceWith(new File(["bytes"], "better.flac")),
		);
		await act(() => result.current.confirm());

		expect(result.current.step).toBe("completed");
		expect(result.current.error).toBeNull();
		expect(
			queryClient.getQueryState(["library", "search", "old credit"])
				?.isInvalidated,
		).toBe(true);
		expect(onReplaced).toHaveBeenCalledWith(track);
	});

	it("releases every unused native selection from the desktop picker", async () => {
		selectDesktopImportFiles.mockResolvedValue([
			{ id: "sel-1", name: "better.flac", size: 10 },
			{ id: "sel-2", name: "extra.flac", size: 10 },
			{ id: "sel-3", name: "extra-2.flac", size: 10 },
		]);
		desktopUploadImportFile.mockResolvedValue(
			new Response(JSON.stringify(awaitingPreview()), { status: 200 }),
		);
		const { result } = renderHook(() => useTrackReplacementFlow(), {
			wrapper,
		});

		act(() => result.current.open(track));
		await act(() => result.current.selectDesktopFile());

		expect(desktopUploadImportFile).toHaveBeenCalledWith(
			"sel-1",
			"job-1",
			expect.any(Function),
			expect.any(AbortSignal),
		);
		expect(releaseDesktopImportSelections).toHaveBeenCalledWith([
			"sel-2",
			"sel-3",
		]);
		expect(releaseDesktopImportSelections).toHaveBeenCalledWith(["sel-1"]);
		expect(result.current.step).toBe("review");
	});

	it("uploads, reviews, confirms, and stops the active Player without autoplay", async () => {
		currentTrackId = "t1";
		const onReplaced = vi.fn();
		const { result } = renderHook(() => useTrackReplacementFlow(onReplaced), {
			wrapper,
		});

		act(() => result.current.open(track));
		await act(() =>
			result.current.replaceWith(
				new File(["bytes"], "better.flac", { type: "audio/flac" }),
			),
		);
		await waitFor(() => expect(result.current.step).toBe("review"));
		expect(createTrackReplacement).toHaveBeenCalledWith("t1");
		expect(uploadManagedImportFile).toHaveBeenCalledWith(
			"job-1",
			"better.flac",
			expect.any(File),
			expect.any(Function),
			expect.any(AbortSignal),
		);
		expect(result.current.preview?.confirmationToken).toBe("token-1");

		await act(() => result.current.confirm());
		expect(confirmTrackReplacement).toHaveBeenCalledWith("job-1", 2, "token-1");
		expect(stopPlayback).toHaveBeenCalledTimes(1);
		expect(refreshQueue).toHaveBeenCalled();
		expect(onReplaced).toHaveBeenCalledWith(track);
		expect(result.current.step).toBe("completed");
	});

	it("leaves an unrelated Player running", async () => {
		currentTrackId = "other-track";
		const { result } = renderHook(() => useTrackReplacementFlow(), {
			wrapper,
		});

		act(() => result.current.open(track));
		await act(() =>
			result.current.replaceWith(new File(["bytes"], "better.flac")),
		);
		await act(() => result.current.confirm());

		expect(stopPlayback).not.toHaveBeenCalled();
		expect(result.current.step).toBe("completed");
	});

	it("reports a rejected replacement and keeps the Track untouched", async () => {
		uploadManagedImportFile.mockRejectedValue(
			new Error("File failed the Strict Import Profile at artwork"),
		);
		const { result } = renderHook(() => useTrackReplacementFlow(), {
			wrapper,
		});

		act(() => result.current.open(track));
		await act(() =>
			result.current.replaceWith(new File(["bytes"], "bad.flac")),
		);

		expect(result.current.step).toBe("select");
		expect(result.current.error).toBe(
			"File failed the Strict Import Profile at artwork",
		);
		expect(cancelManagedImport).toHaveBeenCalledWith("job-1");
		expect(confirmTrackReplacement).not.toHaveBeenCalled();
	});

	it("cancels a reviewed replacement by discarding the staged upload", async () => {
		const { result } = renderHook(() => useTrackReplacementFlow(), {
			wrapper,
		});

		act(() => result.current.open(track));
		await act(() =>
			result.current.replaceWith(new File(["bytes"], "better.flac")),
		);
		await act(() => result.current.cancel());

		expect(cancelManagedImport).toHaveBeenCalledWith("job-1");
		expect(result.current.track).toBeNull();
		expect(confirmTrackReplacement).not.toHaveBeenCalled();
	});
});
