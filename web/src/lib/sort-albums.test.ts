import { describe, expect, it } from "vite-plus/test";
import { sortAlbumsByYear } from "./sort-albums";

describe("sortAlbumsByYear", () => {
	it("sorts newest year first then title", () => {
		const sorted = sortAlbumsByYear([
			{
				id: "1",
				title: "B",
				artistId: "a",
				artistName: "Artist",
				albumArtists: [],
				releaseIdentifiers: [],
				genreItems: [],
				year: 2023,
			},
			{
				id: "2",
				title: "A",
				artistId: "a",
				artistName: "Artist",
				albumArtists: [],
				releaseIdentifiers: [],
				genreItems: [],
				year: 2025,
			},
			{
				id: "3",
				title: "C",
				artistId: "a",
				artistName: "Artist",
				albumArtists: [],
				releaseIdentifiers: [],
				genreItems: [],
				year: 2025,
			},
		]);
		expect(sorted.map((album) => album.id)).toEqual(["2", "3", "1"]);
	});
});
