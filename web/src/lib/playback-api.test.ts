import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { externalPlaybackQueryOptions } from "#/lib/external-playback-query";
import { createPlaybackApi } from "#/lib/playback-api";

const mocks = vi.hoisted(() => ({ getExternalTrackPlayback: vi.fn() }));

vi.mock("#/lib/api", () => ({
	apiClient: { getExternalTrackPlayback: mocks.getExternalTrackPlayback },
}));

describe("createPlaybackApi", () => {
	beforeEach(() => {
		mocks.getExternalTrackPlayback.mockReset();
	});

	it("reuses a prefetched External Track resolve when playing", async () => {
		const playback = {
			format: "hls",
			streamPath: "/api/v1/external/tracks/abc/playlist.m3u8",
		};
		mocks.getExternalTrackPlayback.mockResolvedValue(playback);
		const queryClient = new QueryClient();
		const api = createPlaybackApi(queryClient);

		await queryClient.prefetchQuery(externalPlaybackQueryOptions("abc"));
		await expect(api.getExternalTrackPlayback("abc")).resolves.toEqual(
			playback,
		);
		expect(mocks.getExternalTrackPlayback).toHaveBeenCalledTimes(1);

		// Without a prefetch it resolves on demand.
		await expect(api.getExternalTrackPlayback("other")).resolves.toEqual(
			playback,
		);
		expect(mocks.getExternalTrackPlayback).toHaveBeenLastCalledWith(
			"other",
			expect.any(AbortSignal),
		);
	});

	it("joins a prefetch that is still in flight", async () => {
		let finish!: (value: unknown) => void;
		mocks.getExternalTrackPlayback.mockImplementation(
			() =>
				new Promise((resolve) => {
					finish = resolve;
				}),
		);
		const queryClient = new QueryClient();
		const api = createPlaybackApi(queryClient);

		void queryClient.prefetchQuery(externalPlaybackQueryOptions("abc"));
		const played = api.getExternalTrackPlayback("abc");
		finish({ format: "progressive", streamPath: "/s" });
		await expect(played).resolves.toEqual({
			format: "progressive",
			streamPath: "/s",
		});
		expect(mocks.getExternalTrackPlayback).toHaveBeenCalledTimes(1);
	});
});
