import type { Track, TrackList } from "@repo/api-client";
import { getTrackGenreNames } from "#/lib/library-display";

type GenreSummary = {
	name: string;
	trackCount: number;
	tracks: Track[];
};

/** Genres are derived from track metadata: the server has no genre list. */
export function collectGenres(tracks: Track[]): GenreSummary[] {
	const byKey = new Map<string, GenreSummary>();
	for (const track of tracks) {
		for (const genre of getTrackGenreNames(track)) {
			const key = genre.toLowerCase();
			const current = byKey.get(key);
			if (current) {
				current.trackCount += 1;
				current.tracks.push(track);
			} else {
				byKey.set(key, { name: genre, trackCount: 1, tracks: [track] });
			}
		}
	}
	return [...byKey.values()].sort((a, b) =>
		a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
	);
}

/** Query key + fetch shared by the Genres page and library search. */
export const GENRE_SOURCE_QUERY_KEY = ["library", "tracks", "genres"] as const;

const GENRE_SOURCE_PAGE_SIZE = 500;

/** Read every page so shared genre results and counts cover the whole library. */
export async function fetchGenreTracks(
	load: (params: { limit: number; offset: number }) => Promise<TrackList>,
): Promise<TrackList> {
	const items: Track[] = [];
	let total = 0;
	do {
		const page = await load({
			limit: GENRE_SOURCE_PAGE_SIZE,
			offset: items.length,
		});
		total = page.total ?? page.items.length;
		if (page.items.length === 0 && items.length < total) {
			throw new Error(
				"Genre source pagination ended before all tracks were loaded.",
			);
		}
		items.push(...page.items);
	} while (items.length < total);
	return { items, total };
}
