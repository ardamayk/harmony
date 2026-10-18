import { describe, expect, it } from "vite-plus/test";
import {
	getAlbumArtistName,
	getAlbumGenreNames,
	getTrackArtistName,
	getTrackGenreNames,
} from "./library-display";

describe("normalized library display values", () => {
	it("uses ordered Artist relationships instead of parsing legacy display credits", () => {
		expect(
			getTrackArtistName({
				artistName: "Legacy / Guess",
				artists: [
					{ id: "artist-1", name: "Earth, Wind & Fire" },
					{ id: "artist-2", name: "Guest / Artist" },
				],
			}),
		).toBe("Earth, Wind & Fire, Guest / Artist");
		expect(
			getAlbumArtistName({
				artistName: "Legacy Album Artist",
				albumArtists: [{ id: "artist-3", name: "Various Artists" }],
			}),
		).toBe("Various Artists");
	});

	it("uses structured Genres without splitting punctuation inside names", () => {
		expect(
			getTrackGenreNames({
				genre: "Legacy, Guess",
				genres: [{ id: "genre-1", name: "Electronic / Ambient" }],
			}),
		).toEqual(["Electronic / Ambient"]);
		expect(
			getAlbumGenreNames({
				genres: ["Legacy", "Guess"],
				genreItems: [{ id: "genre-2", name: "R&B, Soul" }],
			}),
		).toEqual(["R&B, Soul"]);
	});
});
