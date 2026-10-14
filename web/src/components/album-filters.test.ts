import { describe, expect, it } from "vite-plus/test";
import { collectAlbumGenres, filterAlbums } from "./album-filters";

describe("collectAlbumGenres", () => {
	it("returns unique sorted genres", () => {
		const genres = collectAlbumGenres([
			{
				id: "1",
				title: "A",
				artistId: "a",
				artistName: "Artist",
				albumArtists: [],
				releaseIdentifiers: [],
				genreItems: [
					{ id: "pop", name: "Pop" },
					{ id: "rock", name: "Rock" },
				],
				genres: ["Pop", "Rock"],
			},
			{
				id: "2",
				title: "B",
				artistId: "a",
				artistName: "Artist",
				albumArtists: [],
				releaseIdentifiers: [],
				genreItems: [
					{ id: "pop", name: "pop" },
					{ id: "jazz", name: "Jazz" },
				],
				genres: ["pop", "Jazz"],
			},
		]);
		expect(genres).toEqual(["Jazz", "Pop", "Rock"]);
	});
});

describe("filterAlbums", () => {
	const albums = [
		{
			id: "1",
			title: "Pop Album",
			artistId: "a",
			artistName: "Taylor",
			albumArtists: [],
			releaseIdentifiers: [],
			genreItems: [{ id: "pop", name: "Pop" }],
			genres: ["Pop"],
		},
		{
			id: "2",
			title: "Rock Album",
			artistId: "b",
			artistName: "Weeknd",
			albumArtists: [],
			releaseIdentifiers: [],
			genreItems: [{ id: "rock", name: "Rock" }],
			genres: ["Rock"],
		},
	];

	it("filters by genre", () => {
		const result = filterAlbums(albums, {
			albumQuery: "",
			artistId: "all",
			genre: "Pop",
		});
		expect(result).toHaveLength(1);
		expect(result[0]?.title).toBe("Pop Album");
	});
});
