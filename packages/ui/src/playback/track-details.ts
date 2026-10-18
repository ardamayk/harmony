import type { ArtistCredit, Track } from "@repo/api-client";
import { formatReplayGainAvailability } from "./format-replay-gain";

const LEGACY_GENRE_DELIMITER_PATTERN = /[;/|,]+/;

export type TrackDetailRow = [label: string, value: string];

/**
 * Listener-facing detail rows for a Track. Both the track list "Details"
 * dialog and the Player Bar "Details" dialog render exactly this list, so
 * the two never drift apart.
 */
export function buildTrackDetailRows(track: Track): TrackDetailRow[] {
	return [
		["Title", track.title],
		["Artist", getTrackArtistName(track)],
		[
			"Album Artist",
			(track.albumArtists ?? []).map((artist) => artist.name).join(", "),
		],
		["Album", track.albumTitle],
		["Disc", track.discNo?.toString()],
		["Track", track.trackNo?.toString()],
		["Duration", formatDurationLabel(track.durationMs)],
		["Codec", track.format],
		["Bitrate", formatBitrate(track.bitrateKbps, track.format)],
		["Sample rate", formatSampleRate(track.sampleRateHz)],
		["Bit depth", formatBitDepth(track.bitDepth)],
		[
			"Track ReplayGain",
			formatReplayGainAvailability(
				track.replayGain?.trackGainDb,
				track.replayGain?.trackPeak,
			),
		],
		[
			"Album ReplayGain",
			formatReplayGainAvailability(
				track.replayGain?.albumGainDb,
				track.replayGain?.albumPeak,
			),
		],
		["Genre", getTrackGenreNames(track).join(", ")],
		["Sample format", track.sampleFormat],
		["Revision", track.revision?.toString()],
		["File modified", formatUnixSeconds(track.fileMtime)],
		["Created", formatTimestamp(track.createdAt)],
		["Updated", formatTimestamp(track.updatedAt)],
	].filter((row): row is TrackDetailRow => Boolean(row[1]));
}

export function getTrackArtistName(
	track: Pick<Track, "artistName" | "artists">,
): string {
	const credits = track.artists;
	if (!credits || credits.length === 0) return track.artistName;
	return credits.map((credit) => credit.name).join(", ");
}

/**
 * Real (identity-backed) artist credits on a Track. Legacy string credits carry
 * a `legacy-artist:` id and cannot be navigated to, so they never appear in
 * "Go to artist" menus.
 */
export function getTrackArtistCredits(
	track: Pick<Track, "artists">,
): ArtistCredit[] {
	return (track.artists ?? []).filter(
		(artist) => artist.id && !artist.id.startsWith("legacy-artist:"),
	);
}

/** Navigate to the Track library filtered to one artist credit. */
export function goToArtistCreditsSearch(
	navigate: (options: {
		to: string;
		search: Record<string, unknown>;
	}) => Promise<void> | void,
	artistId: string,
): Promise<void> | void {
	return navigate({
		to: "/library/tracks",
		search: { artistId },
	});
}

export function getTrackGenreNames(
	track: Pick<Track, "genre" | "genres">,
): string[] {
	if (track.genres !== undefined) {
		return track.genres.map((genre) => genre.name);
	}
	return splitLegacyGenres(track.genre);
}

function splitLegacyGenres(value?: string): string[] {
	if (!value) return [];
	const genres: string[] = [];
	const seen = new Set<string>();
	for (const part of value.split(LEGACY_GENRE_DELIMITER_PATTERN)) {
		const genre = part.trim();
		const key = genre.toLocaleLowerCase();
		if (!genre || seen.has(key)) continue;
		seen.add(key);
		genres.push(genre);
	}
	return genres;
}

export function formatSampleRate(hz?: number): string | null {
	if (!hz || hz <= 0) return null;
	if (hz % 1000 === 0) return `${hz / 1000} kHz`;
	return `${(hz / 1000).toFixed(1)} kHz`;
}

export function formatBitDepth(bits?: number): string | null {
	if (!bits || bits <= 0) return null;
	return `${bits}-bit`;
}

function formatBitrate(kbps?: number, format?: string): string | null {
	if (!kbps || kbps <= 0) return null;
	const provenance =
		format?.toLowerCase() === "wav" ? "Native" : "Calculated by app";
	return `${kbps} kbps (${provenance})`;
}

function formatDurationLabel(ms?: number): string | null {
	if (!ms || ms <= 0) return null;
	const total = Math.floor(ms / 1000);
	const minutes = Math.floor(total / 60);
	const seconds = total % 60;
	return `${minutes}m ${seconds}s`;
}

function formatUnixSeconds(seconds?: number): string | undefined {
	if (!seconds) return undefined;
	return new Date(seconds * 1000).toISOString();
}

function formatTimestamp(value?: string): string | undefined {
	if (!value) return undefined;
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? value : parsed.toISOString();
}
