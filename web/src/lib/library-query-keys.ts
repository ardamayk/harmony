export const libraryQueryKeys = {
	root: ["library"] as const,
	searches: ["library", "search"] as const,
	search: (query: string) => ["library", "search", query] as const,
	artists: (search = "") => ["library", "artists", search] as const,
	artistsAll: ["library", "artists", "all"] as const,
	albums: (search = "", artistId = "") =>
		["library", "albums", search, artistId] as const,
	albumsGenreSource: ["library", "albums", "genre-source"] as const,
	albumsByArtist: (artistId: string) =>
		["library", "albums", "artist", artistId] as const,
	album: (albumId: string) => ["library", "album", albumId] as const,
	tracks: (search = "", artistId = "") =>
		["library", "tracks", search, artistId] as const,
};
