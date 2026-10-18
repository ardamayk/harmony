import type { Track } from "@repo/api-client";
import { getTrackGenreNames } from "./library-display";

export function trackHasGenre(track: Track, genre: string): boolean {
	const target = genre.toLowerCase();
	return getTrackGenreNames(track).some(
		(item) => item.toLowerCase() === target,
	);
}
