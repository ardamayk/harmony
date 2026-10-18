import type { Track } from "@repo/api-client";

/** Oldest addition first; Tracks without a timestamp keep their given order at the end. */
export function sortTracksByAddedAt(tracks: Track[]): Track[] {
	return tracks
		.map((track, index) => ({ track, index }))
		.sort((first, second) => {
			const a = Date.parse(first.track.createdAt ?? "");
			const b = Date.parse(second.track.createdAt ?? "");
			const aValid = Number.isFinite(a);
			const bValid = Number.isFinite(b);
			if (aValid && bValid && a !== b) return a - b;
			if (aValid !== bValid) return aValid ? -1 : 1;
			return first.index - second.index;
		})
		.map(({ track }) => track);
}
