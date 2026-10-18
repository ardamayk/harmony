type AlbumExternalLink = {
	id: string;
	name: string;
	/** Shown when no icon file is available yet */
	short: string;
	/** Static files live in web/public/icons/ */
	iconSrc?: string;
	/** Optional per-icon sizing tweaks (e.g. cropped PNG logos) */
	iconClassName?: string;
	buildUrl: (artist: string, title: string) => string;
	/** The Artist's page, or an artist search, on the same service. */
	buildArtistUrl: (artist: string) => string;
};

export const albumExternalLinks: AlbumExternalLink[] = [
	{
		id: "spotify",
		name: "Spotify",
		short: "SP",
		iconSrc: "/icons/spotify.svg",
		buildUrl: (artist, title) =>
			`https://open.spotify.com/search/${encodeURIComponent(`${artist} ${title}`)}`,
		buildArtistUrl: (artist) =>
			`https://open.spotify.com/search/${encodeURIComponent(artist)}/artists`,
	},
	{
		id: "qobuz",
		name: "Qobuz",
		short: "QZ",
		iconSrc: "/icons/qobuz.png",
		iconClassName: "size-9",
		buildUrl: (artist, title) =>
			`https://www.qobuz.com/us-en/search/albums/${encodeURIComponent(`${artist} ${title}`)}`,
		buildArtistUrl: (artist) =>
			`https://www.qobuz.com/us-en/search/artists/${encodeURIComponent(artist)}`,
	},
	{
		id: "lastfm",
		name: "Last.fm",
		short: "LF",
		iconSrc: "/icons/last-fm.svg",
		buildUrl: (artist, title) =>
			`https://www.last.fm/music/${encodeURIComponent(artist)}/${encodeURIComponent(title)}`,
		buildArtistUrl: (artist) =>
			`https://www.last.fm/music/${encodeURIComponent(artist)}`,
	},
	{
		id: "listenbrainz",
		name: "ListenBrainz",
		short: "LB",
		iconSrc: "/icons/listenbrainz.svg",
		buildUrl: (artist, title) =>
			`https://listenbrainz.org/search/?search_term=${encodeURIComponent(`${artist} ${title}`)}&search_type=artist`,
		buildArtistUrl: (artist) =>
			`https://listenbrainz.org/search/?search_term=${encodeURIComponent(artist)}&search_type=artist`,
	},
];

export function getAlbumExternalLinks(artist: string, title: string) {
	return albumExternalLinks.map((link) => ({
		...link,
		href: link.buildUrl(artist, title),
	}));
}

export function getArtistExternalLinks(artist: string) {
	return albumExternalLinks.map((link) => ({
		...link,
		href: link.buildArtistUrl(artist),
	}));
}
