import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { toast } from "@repo/ui";
import { useDeleteAlbum, useDeleteTrack } from "./use-delete-library";

const playback = vi.hoisted(() => ({
	currentTrack: null as { id: string } | null,
	refreshQueue: async (): Promise<void> => {
		throw new Error("offline");
	},
	clearQueue: async () => {},
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		deleteTrack: vi.fn(async () => ({})),
		deleteAlbum: vi.fn(async () => ({
			deleted: [{ trackId: "deleted" }],
			stoppedAt: null,
		})),
	},
}));
vi.mock("@repo/ui", () => ({
	toast: { success: vi.fn(), error: vi.fn() },
	usePlayback: () => playback,
}));
vi.mock("@tanstack/react-router", () => ({
	useLocation: () => ({ pathname: "/library" }),
	useNavigate: () => vi.fn(),
}));
afterEach(cleanup);

it.each([useDeleteTrack, useDeleteAlbum])(
	"invalidates search after deletion even when playback refresh fails (%#)",
	async (useDelete) => {
		const client = new QueryClient({
			defaultOptions: { queries: { retry: false } },
		});
		const key = ["library", "search", "deleted"];
		client.setQueryData(key, { tracks: [] });
		const { result } = renderHook(() => useDelete(), {
			wrapper: ({ children }) => (
				<QueryClientProvider client={client}>{children}</QueryClientProvider>
			),
		});
		await act(async () => {
			await result.current
				.mutateAsync({
					trackId: "deleted",
					albumId: "album",
					confirmationToken: "confirmed",
				})
				.catch(() => {});
		});
		expect(client.getQueryState(key)?.isInvalidated).toBe(true);
	},
);

it("reports a deleted Album when its Queue item is already gone", async () => {
	playback.currentTrack = { id: "deleted" };
	playback.refreshQueue = async () => {};
	playback.clearQueue = async () => {
		throw new Error("queue item not found");
	};
	const client = new QueryClient();
	const { result } = renderHook(() => useDeleteAlbum(), {
		wrapper: ({ children }) => (
			<QueryClientProvider client={client}>{children}</QueryClientProvider>
		),
	});

	await act(async () => {
		await result.current.mutateAsync({
			albumId: "album",
			confirmationToken: "confirmed",
		});
	});

	expect(toast.success).toHaveBeenCalledWith(
		"All tracks deleted",
		expect.anything(),
	);
	expect(toast.error).not.toHaveBeenCalled();
});
