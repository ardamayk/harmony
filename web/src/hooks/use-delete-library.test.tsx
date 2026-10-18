import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useDeleteAlbum, useDeleteTrack } from "./use-delete-library";

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
	usePlayback: () => ({
		refreshQueue: async () => {
			throw new Error("offline");
		},
	}),
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
