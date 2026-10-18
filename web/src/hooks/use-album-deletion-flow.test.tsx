import type { AlbumDeletionPreview } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useAlbumDeletionFlow } from "./use-album-deletion-flow";

const previewAlbumDeletion = vi.fn();
const deleteAlbum = vi.fn();
const clearQueue = vi.fn(async () => {});
const refreshQueue = vi.fn(async () => {});
const toastSuccess = vi.fn();
const toastError = vi.fn();
const navigate = vi.fn();
let currentTrackId: string | null = null;

vi.mock("#/lib/api", () => ({
	apiClient: {
		previewAlbumDeletion: (...args: unknown[]) => previewAlbumDeletion(...args),
		deleteAlbum: (...args: unknown[]) => deleteAlbum(...args),
	},
}));

vi.mock("@repo/ui", () => ({
	usePlayback: () => ({
		currentTrack: currentTrackId ? { id: currentTrackId } : null,
		clearQueue,
		refreshQueue,
	}),
	toast: {
		success: (...args: unknown[]) => toastSuccess(...args),
		error: (...args: unknown[]) => toastError(...args),
	},
}));

vi.mock("@tanstack/react-router", () => ({
	useNavigate: () => navigate,
	useLocation: () => ({ pathname: "/library/albums" }),
}));

const preview: AlbumDeletionPreview = {
	albumId: "album-1",
	albumTitle: "1989",
	trackCount: 2,
	totalSizeBytes: 2048,
	tracks: [
		{ trackId: "track-1", trackTitle: "Welcome", sizeBytes: 1024 },
		{ trackId: "track-2", trackTitle: "Style", sizeBytes: 1024 },
	],
	playlistReferences: [],
	queueReferences: [{ userId: "user-1", itemCount: 1 }],
	confirmationToken: "token-1",
};

function createWrapper() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	return ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
	);
}

beforeEach(() => {
	vi.clearAllMocks();
	currentTrackId = null;
	previewAlbumDeletion.mockResolvedValue(preview);
});

describe("useAlbumDeletionFlow", () => {
	it("loads the album preview on open and confirms with its token", async () => {
		deleteAlbum.mockResolvedValue({
			deleted: preview.tracks,
			stoppedAt: null,
			deletedFiles: 2,
		});
		const onDeleted = vi.fn();
		const { result } = renderHook(() => useAlbumDeletionFlow(onDeleted), {
			wrapper: createWrapper(),
		});

		act(() => result.current.open({ id: "album-1", title: "1989" }));
		await waitFor(() => expect(result.current.preview).toEqual(preview));
		expect(previewAlbumDeletion).toHaveBeenCalledWith("album-1");

		act(() => result.current.confirm());
		await waitFor(() => expect(result.current.album).toBeNull());
		expect(deleteAlbum).toHaveBeenCalledWith("album-1", "token-1");
		expect(onDeleted).toHaveBeenCalledWith({ id: "album-1", title: "1989" });
		expect(refreshQueue).toHaveBeenCalled();
		expect(toastSuccess).toHaveBeenCalledWith(
			"All tracks deleted",
			expect.objectContaining({ description: "2 tracks deleted" }),
		);
	});

	it("clears the Queue when the playing Track was among the deleted ones", async () => {
		currentTrackId = "track-2";
		deleteAlbum.mockResolvedValue({
			deleted: preview.tracks,
			stoppedAt: null,
			deletedFiles: 2,
		});
		const { result } = renderHook(() => useAlbumDeletionFlow(), {
			wrapper: createWrapper(),
		});
		act(() => result.current.open({ id: "album-1", title: "1989" }));
		await waitFor(() => expect(result.current.preview).toEqual(preview));

		act(() => result.current.confirm());
		await waitFor(() => expect(result.current.album).toBeNull());
		expect(clearQueue).toHaveBeenCalled();
		expect(refreshQueue).not.toHaveBeenCalled();
	});

	it("reports a partial run as a stopped deletion, not a success", async () => {
		deleteAlbum.mockResolvedValue({
			deleted: [preview.tracks[0]],
			stoppedAt: {
				trackId: "track-2",
				trackTitle: "Style",
				reason: "file changed",
			},
			deletedFiles: 1,
		});
		const { result } = renderHook(() => useAlbumDeletionFlow(), {
			wrapper: createWrapper(),
		});
		act(() => result.current.open({ id: "album-1", title: "1989" }));
		await waitFor(() => expect(result.current.preview).toEqual(preview));

		act(() => result.current.confirm());
		await waitFor(() => expect(toastError).toHaveBeenCalled());
		expect(toastError).toHaveBeenCalledWith(
			"Deletion stopped",
			expect.objectContaining({
				description: '1 track deleted; stopped at "Style": file changed',
			}),
		);
		expect(toastSuccess).not.toHaveBeenCalled();
		expect(navigate).not.toHaveBeenCalled();
	});

	it("shows a preview failure inline and never enables confirmation", async () => {
		previewAlbumDeletion.mockRejectedValue(new Error("album not found"));
		const { result } = renderHook(() => useAlbumDeletionFlow(), {
			wrapper: createWrapper(),
		});
		act(() => result.current.open({ id: "album-1", title: "1989" }));
		await waitFor(() => expect(result.current.error).toBe("album not found"));
		expect(result.current.preview).toBeNull();

		act(() => result.current.confirm());
		expect(deleteAlbum).not.toHaveBeenCalled();
	});

	it("surfaces a rejected deletion as a toast and keeps the dialog open", async () => {
		deleteAlbum.mockRejectedValue(new Error("album deletion preview changed"));
		const { result } = renderHook(() => useAlbumDeletionFlow(), {
			wrapper: createWrapper(),
		});
		act(() => result.current.open({ id: "album-1", title: "1989" }));
		await waitFor(() => expect(result.current.preview).toEqual(preview));

		act(() => result.current.confirm());
		await waitFor(() =>
			expect(result.current.error).toBe("album deletion preview changed"),
		);
		expect(toastError).toHaveBeenCalledWith(
			"Tracks could not be deleted",
			expect.objectContaining({
				description: "album deletion preview changed",
			}),
		);
		expect(result.current.album).toEqual({ id: "album-1", title: "1989" });
	});
});
