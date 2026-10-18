import { describe, expect, it } from "vite-plus/test";
import {
	albumExternalLinks,
	getArtistExternalLinks,
} from "./album-external-links";

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

	it("links each service to the Artist", () => {
		const links = Object.fromEntries(
			getArtistExternalLinks("Mavi Gri").map((link) => [link.id, link.href]),
		);
		expect(links).toEqual({
			spotify: "https://open.spotify.com/search/Mavi%20Gri/artists",
			qobuz: "https://www.qobuz.com/us-en/search/artists/Mavi%20Gri",
			lastfm: "https://www.last.fm/music/Mavi%20Gri",
			listenbrainz:
				"https://listenbrainz.org/search/?search_term=Mavi%20Gri&search_type=artist",
		});
	});
});
