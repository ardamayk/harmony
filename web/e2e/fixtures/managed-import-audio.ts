import { deflateSync } from "node:zlib";
import { STRICT_MP3_AUDIO_BASE64 } from "./strict-mp3-audio.ts";

// Deterministic Strict Import Profile fixtures for the Playwright journey.
// The builder mirrors server/testdata/strict-import.mp3: an ID3v2.4 tag with
// every required identity frame plus an embedded PNG front cover, followed by
// the shared MPEG-2 Layer III audio frames rewritten as VBR.

export interface StrictTags {
	title: string;
	artists: string[];
	albumArtist: string;
	album: string;
	track: string;
	disc?: string;
	genres: string[];
	date?: string;
}

export interface FixtureOptions {
	/** Drop the identity frames listed here to produce a rejected file. */
	omitFrames?: Array<"TIT2" | "TPE1" | "TPE2" | "TALB" | "TRCK" | "TCON">;
	/** Omit the optional embedded front cover. */
	omitArtwork?: boolean;
	/** Cover fill colour; a different colour yields a different artwork hash. */
	coverColor?: [number, number, number];
	/** Extra TXXX frames; changing them changes the full-file SHA-256. */
	userText?: Record<string, string>;
}

function syncSafe(value: number): Buffer {
	return Buffer.from([
		(value >> 21) & 0x7f,
		(value >> 14) & 0x7f,
		(value >> 7) & 0x7f,
		value & 0x7f,
	]);
}

function frame(name: string, payload: Buffer): Buffer {
	return Buffer.concat([
		Buffer.from(name, "latin1"),
		syncSafe(payload.length),
		Buffer.from([0, 0]),
		payload,
	]);
}

function textFrame(name: string, value: string): Buffer {
	// Encoding 3 = UTF-8 (ID3v2.4).
	return frame(
		name,
		Buffer.concat([Buffer.from([3]), Buffer.from(value, "utf8")]),
	);
}

function userTextFrame(description: string, value: string): Buffer {
	return frame(
		"TXXX",
		Buffer.concat([
			Buffer.from([3]),
			Buffer.from(description, "utf8"),
			Buffer.from([0]),
			Buffer.from(value, "utf8"),
		]),
	);
}

function artworkFrame(png: Buffer): Buffer {
	return frame(
		"APIC",
		Buffer.concat([
			Buffer.from([0]),
			Buffer.from("image/png", "latin1"),
			Buffer.from([0]),
			// Picture type 3 = front cover, empty description.
			Buffer.from([3, 0]),
			png,
		]),
	);
}

const CRC_TABLE = (() => {
	const table = new Uint32Array(256);
	for (let index = 0; index < 256; index++) {
		let value = index;
		for (let bit = 0; bit < 8; bit++) {
			value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
		}
		table[index] = value >>> 0;
	}
	return table;
})();

function crc32(bytes: Buffer): number {
	let crc = 0xffffffff;
	for (const byte of bytes) {
		crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
	}
	return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type: string, data: Buffer): Buffer {
	const length = Buffer.alloc(4);
	length.writeUInt32BE(data.length);
	const typed = Buffer.concat([Buffer.from(type, "latin1"), data]);
	const crc = Buffer.alloc(4);
	crc.writeUInt32BE(crc32(typed));
	return Buffer.concat([length, typed, crc]);
}

/** A 32x32 solid RGBA PNG, matching the server fixture's cover. */
export function coverPng(
	[red, green, blue]: [number, number, number] = [12, 98, 180],
): Buffer {
	const size = 32;
	const header = Buffer.alloc(13);
	header.writeUInt32BE(size, 0);
	header.writeUInt32BE(size, 4);
	header[8] = 8; // bit depth
	header[9] = 6; // RGBA
	const rows: Buffer[] = [];
	for (let y = 0; y < size; y++) {
		const row = Buffer.alloc(1 + size * 4);
		for (let x = 0; x < size; x++) {
			row.set([red, green, blue, 255], 1 + x * 4);
		}
		rows.push(row);
	}
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		pngChunk("IHDR", header),
		pngChunk("IDAT", deflateSync(Buffer.concat(rows))),
		pngChunk("IEND", Buffer.alloc(0)),
	]);
}

function mpeg2Layer3FrameSize(frame: Buffer): number {
	const padding = (frame[2] >> 1) & 1;
	return Math.floor((72 * 48_000) / 22_050) + padding;
}

function vbrAudio(): Buffer {
	const audio = Buffer.from(STRICT_MP3_AUDIO_BASE64, "base64");
	const frames: Buffer[] = [];
	for (let frameIndex = 0, offset = 0; offset < audio.length; frameIndex++) {
		const size = mpeg2Layer3FrameSize(audio.subarray(offset));
		let current = Buffer.from(audio.subarray(offset, offset + size));
		if (frameIndex % 2 === 1) {
			current[2] = (current[2] & 0x0f) | 0x70;
			current = Buffer.concat([current, Buffer.alloc(26)]);
		}
		frames.push(current);
		offset += size;
	}
	return Buffer.concat(frames);
}

export function buildStrictMp3(
	tags: StrictTags,
	options: FixtureOptions = {},
): Buffer {
	const omit = new Set(options.omitFrames ?? []);
	const frames: Buffer[] = [];
	if (!omit.has("TIT2")) frames.push(textFrame("TIT2", tags.title));
	if (!omit.has("TPE1"))
		frames.push(textFrame("TPE1", tags.artists.join("\0")));
	if (!omit.has("TPE2")) frames.push(textFrame("TPE2", tags.albumArtist));
	if (!omit.has("TALB")) frames.push(textFrame("TALB", tags.album));
	if (!omit.has("TRCK")) frames.push(textFrame("TRCK", tags.track));
	frames.push(textFrame("TPOS", tags.disc ?? "1/1"));
	if (!omit.has("TCON")) frames.push(textFrame("TCON", tags.genres.join("\0")));
	frames.push(textFrame("TDRC", tags.date ?? "2026-08-31"));
	for (const [description, value] of Object.entries(options.userText ?? {})) {
		frames.push(userTextFrame(description, value));
	}
	if (!options.omitArtwork) {
		frames.push(artworkFrame(coverPng(options.coverColor)));
	}
	const payload = Buffer.concat(frames);
	const tag = Buffer.concat([
		Buffer.from("ID3", "latin1"),
		Buffer.from([4, 0, 0]),
		syncSafe(payload.length),
		payload,
	]);
	return Buffer.concat([tag, vbrAudio()]);
}
