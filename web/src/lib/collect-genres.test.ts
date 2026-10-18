import type { Track, TrackList } from "@repo/api-client";
import { describe, expect, it, vi } from "vite-plus/test";
import { collectGenres, fetchGenreTracks } from "./collect-genres";

function createTrack(index: number, genres = ["Shared"]): Track {
	return {
		id: `track-${index}`,
		title: `Track ${index}`,
		artistName: "Artist",
		artists: [],
		albumId: "album",
		discNo: 1,
		durationMs: 1000,
		format: "flac",
		genres: genres.map((name) => ({ id: name.toLowerCase(), name })),
	};
}

describe("fetchGenreTracks", () => {
	it("includes genres and accurate counts beyond the first 500 tracks", async () => {
		const firstPage = Array.from({ length: 500 }, (_, index) =>
			createTrack(index),
		);
		const load = vi
			.fn<Parameters<typeof fetchGenreTracks>[0]>()
			.mockResolvedValueOnce({ items: firstPage, total: 501 })
			.mockResolvedValueOnce({
				items: [createTrack(500, ["Shared", "Late Genre"])],
				total: 501,
			});
		const result = await fetchGenreTracks(load);
		expect(load.mock.calls).toEqual([
			[{ limit: 500, offset: 0 }],
			[{ limit: 500, offset: 500 }],
		]);
		expect(
			collectGenres(result.items).map(({ name, trackCount }) => ({
				name,
				trackCount,
			})),
		).toEqual([
			{ name: "Late Genre", trackCount: 1 },
			{ name: "Shared", trackCount: 501 },
		]);
	});

	it("does not publish partial results when a later page fails", async () => {
		const load = vi
			.fn<Parameters<typeof fetchGenreTracks>[0]>()
			.mockResolvedValueOnce({ items: [createTrack(0)], total: 2 })
			.mockRejectedValueOnce(new Error("Connection lost"));
		await expect(fetchGenreTracks(load)).rejects.toThrow("Connection lost");
	});

	it("rejects an incomplete empty page instead of looping or truncating", async () => {
		const load = vi.fn(async (): Promise<TrackList> => ({
			items: [],
			total: 1,
		}));
		await expect(fetchGenreTracks(load)).rejects.toThrow("pagination ended");
		expect(load).toHaveBeenCalledTimes(1);
	});
});
