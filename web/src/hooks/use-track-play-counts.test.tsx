import type { Track } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it, vi } from "vite-plus/test";
import { useTrackPlayCounts } from "./use-track-play-counts";

const listTrackPlayCounts = vi.fn(
	async (ids: string[]) => new Map(ids.map((id) => [id, Number(id)])),
);

vi.mock("#/lib/api", () => ({
	apiClient: {
		listTrackPlayCounts: (ids: string[]) => listTrackPlayCounts(ids),
	},
}));

it("splits more than 200 Tracks into requests the server accepts", async () => {
	const tracks = Array.from({ length: 450 }, (_, i) => ({
		id: String(i),
	})) as Track[];
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	const wrapper = ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
	);
	const { result } = renderHook(() => useTrackPlayCounts(tracks), { wrapper });
	await waitFor(() => expect(result.current.size).toBe(450));
	expect(listTrackPlayCounts.mock.calls.map(([ids]) => ids.length)).toEqual([
		200, 200, 50,
	]);
	expect(result.current.get("449")).toBe(449);
});
