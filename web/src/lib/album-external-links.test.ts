import { describe, expect, it } from "vite-plus/test";
import { albumExternalLinks } from "./album-external-links";

const artist = "Taylor Swift";
const title = "The Life of a Showgirl";

describe("albumExternalLinks", () => {
	it("builds Spotify search URL", () => {
		const spotify = albumExternalLinks.find((link) => link.id === "spotify");
		expect(spotify?.buildUrl(artist, title)).toBe(
			"https://open.spotify.com/search/Taylor%20Swift%20The%20Life%20of%20a%20Showgirl",
		);
	});

	it("builds Last.fm album path URL", () => {
		const lastfm = albumExternalLinks.find((link) => link.id === "lastfm");
		expect(lastfm?.buildUrl(artist, title)).toBe(
			"https://www.last.fm/music/Taylor%20Swift/The%20Life%20of%20a%20Showgirl",
		);
	});
});
