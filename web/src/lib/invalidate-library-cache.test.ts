import type { AlbumDetail, AlbumList } from "@repo/api-client";
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vite-plus/test";
import { invalidateLibraryCache } from "./invalidate-library-cache";

describe("invalidateLibraryCache", () => {
	it("removes deleted album from cached album lists immediately", async () => {
		const queryClient = new QueryClient();
		queryClient.setQueryData<AlbumList>(["library", "albums", "", "all"], {
			items: [
				{
					id: "album-1",
					title: "Keep",
					artistId: "a1",
					artistName: "Artist",
					albumArtists: [],
					releaseIdentifiers: [],
					genreItems: [],
				},
				{
					id: "album-2",
					title: "Delete me",
					artistId: "a1",
					artistName: "Artist",
					albumArtists: [],
					releaseIdentifiers: [],
					genreItems: [],
				},
			],
			total: 2,
		});

		const invalidateSpy = vi
			.spyOn(queryClient, "invalidateQueries")
			.mockResolvedValue(undefined as never);

		await invalidateLibraryCache(queryClient, { albumId: "album-2" });

		const cached = queryClient.getQueryData<AlbumList>([
			"library",
			"albums",
			"",
			"all",
		]);
		expect(cached?.items).toHaveLength(1);
		expect(cached?.items[0]?.id).toBe("album-1");
		expect(cached?.total).toBe(1);
		expect(
			queryClient.getQueryState(["library", "album", "album-2"]),
		).toBeUndefined();
		expect(invalidateSpy).toHaveBeenCalledWith({
			queryKey: ["library"],
		});
		// Inactive search entries only go stale; they must not be refetched.
		expect(invalidateSpy).toHaveBeenCalledWith({
			queryKey: ["library", "search"],
			refetchType: "none",
		});
	});

	it("removes deleted track from cached album detail immediately", async () => {
		const queryClient = new QueryClient();
		vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(
			undefined as never,
		);
		queryClient.setQueryData<AlbumDetail>(["library", "album", "album-1"], {
			id: "album-1",
			title: "Album",
			artistId: "a1",
			artistName: "Artist",
			albumArtists: [],
			releaseIdentifiers: [],
			genreItems: [],
			trackCount: 2,
			tracks: [
				{
					id: "track-1",
					title: "Keep",
					artistName: "Artist",
					artists: [],
					albumId: "album-1",
					discNo: 1,
					durationMs: 1000,
					format: "flac",
					genres: [],
				},
				{
					id: "track-2",
					title: "Delete",
					artistName: "Artist",
					artists: [],
					albumId: "album-1",
					discNo: 1,
					durationMs: 1000,
					format: "flac",
					genres: [],
				},
			],
		});

		await invalidateLibraryCache(queryClient, { trackId: "track-2" });

		const cached = queryClient.getQueryData<AlbumDetail>([
			"library",
			"album",
			"album-1",
		]);
		expect(cached?.tracks).toHaveLength(1);
		expect(cached?.tracks[0]?.id).toBe("track-1");
		expect(cached?.trackCount).toBe(1);
	});
});
