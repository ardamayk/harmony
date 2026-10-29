import {
	type AcquisitionQuality,
	type AcquisitionScope,
	ApiError,
	type ExternalTrack,
	type SoulseekAcquisition,
} from "@repo/api-client";

export const soulseekQueryKeys = {
	status: ["soulseek", "status"] as const,
	search: (searchId: string) => ["soulseek", "search", searchId] as const,
	acquisitions: ["soulseek", "acquisitions"] as const,
};

/** Bracketed video-site decorations that never name a release. */
const TITLE_NOISE =
	/\s*[([][^)\]]*\b(?:official|video|audio|lyrics?|visuali[sz]er|hd|hq|4k|mv)\b[^)\]]*[)\]]/gi;

/** Channel names video sites append to an artist. */
function cleanArtistName(name: string): string {
	return name
		.replace(/\s+-\s+topic$/i, "")
		.replace(/vevo$/i, "")
		.trim();
}

/** Query words without decorations, separators or extra spaces. */
function cleanQueryWords(words: string): string {
	// A lone dash would read as Soulseek's exclusion operator.
	return words
		.replace(TITLE_NOISE, "")
		.replace(/\s+[-–—|]\s+/g, " ")
		.replace(/\s+/g, " ")
		.trim();
}

/**
 * The Soulseek query for an External Track, without the decorations a video
 * title carries such as "(Official Video)". The Album scope searches artist
 * and Album when the site reports the Album; otherwise, and for the Track
 * scope, artist and title. The listener edits the query before searching.
 */
export function buildSoulseekQuery(
	track: ExternalTrack,
	scope: AcquisitionScope = "track",
): string {
	const album =
		scope === "album" && track.album ? cleanQueryWords(track.album) : "";
	const name = album || cleanQueryWords(track.title);
	const artist = track.artistName ? cleanArtistName(track.artistName) : "";
	// Video titles often read "Artist - Title" already.
	if (!artist || name.toLowerCase().startsWith(artist.toLowerCase()))
		return name;
	return `${artist} ${name}`;
}

function formatSampleRate(hz: number): string {
	const khz = hz / 1000;
	return `${Number.isInteger(khz) ? khz : khz.toFixed(1)} kHz`;
}

/**
 * The quality an Acquisition Version claims, as its sources report it;
 * anything missing reads "unknown" and is never guessed. A variable
 * bitrate reads as the level the server grouped the sources by, such as
 * "MP3 VBR ~245 kbps".
 */
export function formatAcquisitionQuality(quality: AcquisitionQuality): string {
	const format = quality.format.toUpperCase();
	if (quality.lossless) {
		if (quality.bitDepth == null && quality.sampleRateHz == null)
			return `${format}, quality unknown`;
		const depth =
			quality.bitDepth != null
				? `${quality.bitDepth}-bit`
				: "unknown bit depth";
		const rate =
			quality.sampleRateHz != null
				? formatSampleRate(quality.sampleRateHz)
				: "unknown sample rate";
		return `${format} ${depth} / ${rate}`;
	}
	const kind = quality.variableBitrate ? `${format} VBR` : format;
	if (quality.bitrateKbps == null) return `${kind}, bitrate unknown`;
	// A variable bitrate is the level of the sources' average, hence "~".
	return quality.variableBitrate
		? `${kind} ~${quality.bitrateKbps} kbps`
		: `${kind} ${quality.bitrateKbps} kbps`;
}

const BYTES_PER_KIB = 1024;
const BYTES_PER_MIB = 1024 * 1024;
const BYTES_PER_GIB = 1024 * 1024 * 1024;

export function formatSize(bytes: number): string {
	if (bytes >= BYTES_PER_GIB)
		return `${(bytes / BYTES_PER_GIB).toFixed(2)} GiB`;
	return `${(bytes / BYTES_PER_MIB).toFixed(1)} MiB`;
}

export function formatSpeed(bytesPerSecond: number): string {
	if (bytesPerSecond >= BYTES_PER_MIB)
		return `${(bytesPerSecond / BYTES_PER_MIB).toFixed(1)} MiB/s`;
	return `${Math.round(bytesPerSecond / BYTES_PER_KIB)} KiB/s`;
}

/** Acquisitions the worker still moves forward; the list polls while any exist. */
export function isActiveAcquisition(acquisition: SoulseekAcquisition): boolean {
	return (
		acquisition.state === "downloading" || acquisition.state === "importing"
	);
}

/** What went wrong with a Soulseek request, and what to do next. */
export function soulseekErrorMessage(error: unknown): string {
	const code = error instanceof ApiError ? error.body.code : undefined;
	switch (code) {
		case "slskd_unreachable":
			return "Couldn't reach slskd. Check that it is running, then retry.";
		case "soulseek_not_logged_in":
			return "slskd isn't logged in to Soulseek. Check its Soulseek account, then retry.";
		case "soulseek_acquisition_disabled":
			return "This Music Server no longer offers Soulseek Acquisition.";
		case "search_not_found":
			return "This search has expired. Search again.";
		case "version_not_found":
			return "This version is no longer offered. Search again.";
		case "acquisition_not_found":
			return "This acquisition no longer exists.";
		default:
			return error instanceof ApiError && error.message.trim()
				? error.message
				: "Soulseek request failed. Retry in a moment.";
	}
}
