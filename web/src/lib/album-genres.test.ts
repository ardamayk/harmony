import { describe, expect, it } from "vite-plus/test";
import { getAlbumGenres } from "./album-genres";

describe("getAlbumGenres", () => {
	it("uses album genres when present", () => {
		const genres = getAlbumGenres({
			id: "a1",
			title: "Album",
			artistId: "ar1",
			artistName: "Artist",
			albumArtists: [],
			releaseIdentifiers: [],
			genreItems: undefined as never,
			tracks: [],
			genres: ["Pop", "Rock"],
		});
		expect(genres).toEqual(["Pop", "Rock"]);
	});

	it("falls back to unique track genres", () => {
		const genres = getAlbumGenres({
			id: "a1",
			title: "Album",
			artistId: "ar1",
			artistName: "Artist",
			albumArtists: [],
			releaseIdentifiers: [],
			genreItems: undefined as never,
			genres: [],
			tracks: [
				{
					id: "t1",
					title: "A",
					artistName: "Artist",
					artists: [],
					albumId: "a1",
					discNo: 1,
					durationMs: 1,
					format: "flac",
					genre: "Pop",
					genres: undefined as never,
				},
				{
					id: "t2",
					title: "B",
					artistName: "Artist",
					artists: [],
					albumId: "a1",
					discNo: 1,
					durationMs: 1,
					format: "flac",
					genre: "pop; Synthpop",
					genres: undefined as never,
				},
			],
		});
		expect(genres).toEqual(["Pop", "Synthpop"]);
	});
});
