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

/**
 * A searched query without the artist. Soulseek's server filters some
 * artist names, so any query naming one finds nothing: this is the Album
 * (Album scope, when the site reports it) or the title, cleaned as in
 * {@link buildSoulseekQuery} and without a leading artist. Empty when the
 * query names no artist to drop.
 */
export function soulseekQueryWithoutArtist(
	track: ExternalTrack,
	scope: AcquisitionScope,
	query: string,
): string {
	const artist = track.artistName
		? cleanArtistName(track.artistName).toLowerCase()
		: "";
	if (!artist || !query.toLowerCase().includes(artist)) return "";
	const album =
		scope === "album" && track.album ? cleanQueryWords(track.album) : "";
	let name = album || cleanQueryWords(track.title);
	if (name.toLowerCase().startsWith(`${artist} `))
		name = name.slice(artist.length).trim();
	return name.toLowerCase() === query.trim().toLowerCase() ? "" : name;
}

function khz(hz: number): string {
	const value = hz / 1000;
	return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function formatSampleRate(hz: number): string {
	return `${khz(hz)} kHz`;
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

/**
 * A quality the listener picks before searching Soulseek, from a fixed list.
 * Formats are the Music Server's, by file extension: AAC is `m4a`, Ogg
 * Vorbis is `ogg`.
 */
export type QualityChoice =
	| { key: string; format: string; lossless: false; bitrateKbps: number }
	| {
			key: string;
			format: string;
			lossless: true;
			bitDepth: number;
			sampleRateHz: number;
	  };

export interface QualityGroup {
	format: string;
	label: string;
	unit: string;
	choices: QualityChoice[];
}

function lossyGroup(
	format: string,
	label: string,
	bitrates: number[],
): QualityGroup {
	return {
		format,
		label,
		unit: "kbps",
		choices: bitrates.map((bitrateKbps) => ({
			key: `${format}-${bitrateKbps}`,
			format,
			lossless: false,
			bitrateKbps,
		})),
	};
}

const LOSSLESS_STEPS: Array<[bitDepth: number, sampleRateHz: number]> = [
	[16, 44_100],
	[16, 48_000],
	[24, 44_100],
	[24, 48_000],
	[24, 88_200],
	[24, 96_000],
	[24, 176_400],
	[24, 192_000],
];

function losslessGroup(format: string, label: string): QualityGroup {
	return {
		format,
		label,
		unit: "bit / kHz",
		choices: LOSSLESS_STEPS.map(([bitDepth, sampleRateHz]) => ({
			key: `${format}-${bitDepth}-${sampleRateHz}`,
			format,
			lossless: true,
			bitDepth,
			sampleRateHz,
		})),
	};
}

/** The qualities the Install dialog offers, by format. */
export const QUALITY_GROUPS: QualityGroup[] = [
	lossyGroup("mp3", "MP3", [128, 192, 256, 320]),
	lossyGroup("m4a", "AAC (M4A)", [128, 192, 256]),
	lossyGroup("opus", "Opus", [96, 128, 160, 192]),
	lossyGroup("ogg", "Ogg Vorbis", [128, 160, 192, 256, 320]),
	losslessGroup("flac", "FLAC"),
	losslessGroup("wav", "WAV"),
];

const QUALITY_CHOICES = new Map(
	QUALITY_GROUPS.flatMap((group) =>
		group.choices.map((choice) => [choice.key, choice] as const),
	),
);

/** A stored choice key back to its choice; null when not on the list. */
export function parseQualityChoice(
	key: string | null | undefined,
): QualityChoice | null {
	return (key && QUALITY_CHOICES.get(key)) || null;
}

/** A choice's value within its group, such as "320" or "24/88.2". */
export function formatQualityChoiceValue(choice: QualityChoice): string {
	return choice.lossless
		? `${choice.bitDepth}/${khz(choice.sampleRateHz)}`
		: String(choice.bitrateKbps);
}

/** A choice with its format, such as "MP3 320 kbps" or "FLAC 24/88.2". */
export function formatQualityChoice(choice: QualityChoice): string {
	const group = QUALITY_GROUPS.find((group) => group.format === choice.format);
	const format = group?.label ?? choice.format.toUpperCase();
	return choice.lossless
		? `${format} ${formatQualityChoiceValue(choice)}`
		: `${format} ${choice.bitrateKbps} kbps`;
}

/**
 * Whether a version's quality is the chosen one: lossless by format, bit
 * depth and sample rate; lossy by format and a bitrate that is not
 * reported as variable.
 */
export function qualityMatches(
	quality: AcquisitionQuality,
	choice: QualityChoice,
): boolean {
	if (quality.format !== choice.format) return false;
	if (choice.lossless)
		return (
			quality.lossless &&
			quality.bitDepth === choice.bitDepth &&
			quality.sampleRateHz === choice.sampleRateHz
		);
	return (
		!quality.lossless &&
		quality.variableBitrate !== true &&
		quality.bitrateKbps === choice.bitrateKbps
	);
}

/** The listed choice a found quality is, or null when it is not listed. */
export function qualityChoiceOf(
	quality: AcquisitionQuality,
): QualityChoice | null {
	for (const choice of QUALITY_CHOICES.values())
		if (qualityMatches(quality, choice)) return choice;
	return null;
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
