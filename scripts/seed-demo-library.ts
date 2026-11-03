// Seeds a Music Server with a small demo library through Managed Import: two
// Albums of melodies synthesized here (no recordings, so no licence question),
// fully tagged, with an embedded front cover and synced LRC lyrics. Albums
// already in the library are skipped, so running it again changes nothing.
//
//   SEED_URL=https://dev.aneomol.uk SEED_BASIC_AUTH=preview:… mise run seed:demo
//
// SEED_USERNAME and SEED_PASSWORD sign in to the Music Server (default: the
// public demo account demo / demo of dev and previews). SEED_BASIC_AUTH
// (user:password) is sent as basic auth for Traefik in front of dev and
// previews. Needs ffmpeg on PATH.
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export type DemoTrack = { title: string; lyrics: string[] };
export type DemoAlbum = {
	title: string;
	artist: string;
	year: number;
	genre: string;
	/** Cover gradient, two ffmpeg colours. */
	colors: [string, string];
	timbre: "soft" | "square";
	bpm: number;
	/** MIDI note of the key's root. */
	root: number;
	tracks: DemoTrack[];
};

export const DEMO_ALBUMS: DemoAlbum[] = [
	{
		title: "First Light",
		artist: "The Sine Waves",
		year: 2026,
		genre: "Ambient",
		colors: ["0x1d4e89", "0xf6a54c"],
		timbre: "soft",
		bpm: 96,
		root: 60,
		tracks: [
			{
				title: "Dawn Chorus",
				lyrics: [
					"The kettle hums before the sun",
					"A window fogs, the day begun",
					"Birds rehearse a borrowed tune",
					"Hold the light, it fades so soon",
				],
			},
			{
				title: "Paper Boats",
				lyrics: [
					"Fold the morning into sails",
					"Send it down the garden rails",
					"Every puddle is a sea",
					"Carry something back to me",
				],
			},
			{
				title: "Slow Orbit",
				lyrics: [
					"Round and round the kitchen light",
					"Moths are planets, small and bright",
					"Gravity is just a chair",
					"Stay a while, the air is fair",
				],
			},
			{
				title: "Last Light",
				lyrics: [
					"Shadows lengthen on the floor",
					"Someone knocking at the door",
					"Close the curtains, keep the glow",
					"Hum the tune we used to know",
				],
			},
		],
	},
	{
		title: "Night Bus",
		artist: "Oscillator Club",
		year: 2025,
		genre: "Synthpop",
		colors: ["0x2b1055", "0x00c2a8"],
		timbre: "square",
		bpm: 120,
		root: 57,
		tracks: [
			{
				title: "Neon Rain",
				lyrics: [
					"Puddles full of signs that blink",
					"Streetlights turning green to pink",
					"Ride the last bus through the town",
					"Never let the music down",
				],
			},
			{
				title: "Terminal Stop",
				lyrics: [
					"Everybody off, the driver said",
					"Count the windows, count the red",
					"Walk the long way, take your time",
					"Every step a little rhyme",
				],
			},
		],
	},
];

const RATE = 44_100;
const SECONDS = 40;
// I–vi–IV–V in semitones above the root, one chord per bar of four beats.
const PROGRESSION = [0, 9, 5, 7];
const PENTATONIC = [0, 2, 4, 7, 9, 12, 14, 16];
const INTRO_BARS = 2;

const hz = (midi: number) => 440 * 2 ** ((midi - 69) / 12);

/** A small seeded PRNG, so every run synthesizes the same bytes. */
function random(seed: number): () => number {
	return () => {
		seed = (seed + 0x6d2b79f5) | 0;
		let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
	};
}

/** Mono 16-bit PCM: a pentatonic melody over a bass line, faded in and out. */
export function synthesize(album: DemoAlbum, trackIndex: number): Int16Array {
	const next = random(album.root * 31 + trackIndex * 7 + album.bpm);
	const beat = 60 / album.bpm;
	const samples = new Float32Array(RATE * SECONDS);
	const voice = (phase: number) =>
		album.timbre === "soft"
			? Math.sin(phase) + 0.25 * Math.sin(2 * phase)
			: Math.sin(phase) + Math.sin(3 * phase) / 3 + Math.sin(5 * phase) / 5;
	const note = (start: number, length: number, freq: number, gain: number) => {
		const from = Math.floor(start * RATE);
		const to = Math.min(samples.length, Math.floor((start + length) * RATE));
		for (let i = from; i < to; i++) {
			const t = (i - from) / RATE;
			const envelope = Math.min(1, t / 0.01) * Math.exp(-3 * t);
			samples[i] += gain * envelope * voice(2 * Math.PI * freq * t);
		}
	};
	let step = Math.floor(next() * 4);
	const beats = Math.floor(SECONDS / beat);
	for (let b = 0; b < beats; b++) {
		const chord = PROGRESSION[Math.floor(b / 4) % PROGRESSION.length];
		if (b % 4 === 0)
			note(b * beat, 4 * beat, hz(album.root - 24 + chord), 0.35);
		if (b < INTRO_BARS * 4) continue;
		step = Math.max(
			0,
			Math.min(PENTATONIC.length - 1, step + Math.floor(next() * 5) - 2),
		);
		note(b * beat, beat, hz(album.root + PENTATONIC[step]), 0.3);
		// Some beats get an off-beat neighbour, so the line is not all quarter notes.
		if (next() < 0.35)
			note(
				(b + 0.5) * beat,
				beat / 2,
				hz(album.root + PENTATONIC[Math.max(0, step - 1)]),
				0.2,
			);
	}
	let peak = 0;
	for (const value of samples) peak = Math.max(peak, Math.abs(value));
	const pcm = new Int16Array(samples.length);
	for (let i = 0; i < samples.length; i++) {
		const fade = Math.min(1, i / RATE, (samples.length - i) / RATE);
		pcm[i] = Math.round((samples[i] / peak) * fade * 0.8 * 32_767);
	}
	return pcm;
}

/** LRC text: one line per bar after the intro, cycling through the verse. */
export function lrc(album: DemoAlbum, track: DemoTrack): string {
	const bar = (4 * 60) / album.bpm;
	const lines: string[] = [];
	for (let b = INTRO_BARS; (b + 1) * bar < SECONDS - 1; b++) {
		const time = b * bar;
		const minutes = String(Math.floor(time / 60)).padStart(2, "0");
		const seconds = (time % 60).toFixed(2).padStart(5, "0");
		lines.push(
			`[${minutes}:${seconds}] ${track.lyrics[(b - INTRO_BARS) % track.lyrics.length]}`,
		);
	}
	return lines.join("\n");
}

function ffmpeg(args: string[], input?: Buffer): void {
	execFileSync("ffmpeg", ["-v", "error", "-y", ...args], {
		input,
		stdio: ["pipe", "inherit", "inherit"],
	});
}

/** Writes the Album's tagged FLAC files into `dir` and returns their paths. */
function buildAlbum(album: DemoAlbum, dir: string): string[] {
	const cover = join(dir, `${album.title}.png`);
	const [c0, c1] = album.colors;
	ffmpeg([
		"-f",
		"lavfi",
		"-i",
		`gradients=s=600x600:c0=${c0}:c1=${c1}:n=2:speed=0:seed=1`,
		"-frames:v",
		"1",
		cover,
	]);
	return album.tracks.map((track, index) => {
		const file = join(
			dir,
			`${album.artist} - ${String(index + 1).padStart(2, "0")} ${track.title}.flac`,
		);
		const tags = {
			TITLE: track.title,
			ARTIST: album.artist,
			ALBUMARTIST: album.artist,
			ALBUM: album.title,
			TRACKNUMBER: `${index + 1}/${album.tracks.length}`,
			DISCNUMBER: "1/1",
			DATE: String(album.year),
			GENRE: album.genre,
			LYRICS: lrc(album, track),
		};
		const pcm = synthesize(album, index);
		ffmpeg(
			[
				...[
					"-f",
					"s16le",
					"-ar",
					String(RATE),
					"-ac",
					"1",
					"-i",
					"pipe:0",
					"-i",
					cover,
				],
				...["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy"],
				...[
					"-disposition:v",
					"attached_pic",
					"-metadata:s:v",
					"comment=Cover (front)",
				],
				...Object.entries(tags).flatMap(([key, value]) => [
					"-metadata",
					`${key}=${value}`,
				]),
				file,
			],
			Buffer.from(pcm.buffer),
		);
		return file;
	});
}

type Album = { id: string; title: string; artistName: string };
type BatchFile = {
	jobId: string;
	state: string;
	outcome?: string;
	errorReason?: string;
	originalFilename?: string;
};
type Batch = { id: string; revision: number; files: BatchFile[] };

function client(
	baseUrl: string,
	username: string,
	password: string,
	basicAuth: string,
) {
	const headers: Record<string, string> = {};
	// Traefik strips Authorization before the Music Server, so dev and previews
	// need both basic auth and a session.
	if (basicAuth)
		headers.authorization = `Basic ${Buffer.from(basicAuth).toString("base64")}`;
	const call = async (path: string, init: RequestInit = {}) => {
		const response = await fetch(`${baseUrl}${path}`, {
			...init,
			headers: { ...headers, ...(init.headers as Record<string, string>) },
		});
		if (!response.ok)
			throw new Error(
				`${init.method ?? "GET"} ${path}: ${response.status} ${await response.text()}`,
			);
		return response;
	};
	const json = async <T>(path: string, init?: RequestInit) =>
		(await call(path, init)).json() as Promise<T>;
	const send = (method: string, body: unknown) => ({
		method,
		headers: { "content-type": "application/json" },
		body: JSON.stringify(body),
	});
	return {
		call,
		json,
		send,
		async signIn() {
			if (!username) return;
			const response = await fetch(`${baseUrl}/api/v1/auth/login`, {
				...send("POST", { username, password }),
				headers: { ...headers, "content-type": "application/json" },
			});
			if (response.status === 404) return; // sign-in is not configured
			if (!response.ok)
				throw new Error(`sign-in: ${response.status} ${await response.text()}`);
			const session = response.headers
				.getSetCookie()
				.find((cookie) => cookie.startsWith("harmony_session="));
			if (!session)
				throw new Error("sign-in answered without a harmony_session cookie");
			headers.cookie = session.split(";")[0];
		},
	};
}

type Client = ReturnType<typeof client>;

async function findAlbum(
	api: Client,
	album: DemoAlbum,
): Promise<Album | undefined> {
	const { items } = await api.json<{ items: Album[] }>(
		`/api/v1/library/albums?limit=50&q=${encodeURIComponent(album.title)}`,
	);
	return items.find(
		(item) => item.title === album.title && item.artistName === album.artist,
	);
}

async function importFiles(api: Client, files: string[]): Promise<void> {
	const created = await api.json<Batch>(
		"/api/v1/import-batches",
		api.send("POST", {}),
	);
	for (const file of files) {
		const job = await api.json<{ id: string }>(
			"/api/v1/imports",
			api.send("POST", { batchId: created.id, clientFileId: randomUUID() }),
		);
		const name = file.split("/").pop() ?? "demo.flac";
		await api.call(`/api/v1/imports/${job.id}/file`, {
			method: "PUT",
			headers: {
				"content-type": "audio/flac",
				"x-import-filename": encodeURIComponent(name),
				"x-import-filename-encoding": "url",
			},
			body: readFileSync(file),
		});
	}
	const batch = await api.json<Batch>(`/api/v1/import-batches/${created.id}`);
	const rejected = batch.files.filter((file) => file.state !== "accepted");
	if (rejected.length) {
		await api.call(`/api/v1/import-batches/${batch.id}`, { method: "DELETE" });
		throw new Error(
			`rejected: ${rejected.map((file) => `${file.originalFilename}: ${file.errorReason}`).join("; ")}`,
		);
	}
	const result = await api.json<Batch>(
		`/api/v1/import-batches/${batch.id}/confirm`,
		api.send("POST", {
			revision: batch.revision,
			selectedFileIds: batch.files.map((file) => file.jobId),
		}),
	);
	const failed = result.files.filter((file) => file.outcome !== "imported");
	if (failed.length) {
		throw new Error(
			`not imported: ${failed.map((file) => `${file.originalFilename}: ${file.outcome} ${file.errorReason ?? ""}`).join("; ")}`,
		);
	}
}

/** Self-check: each demo Album exists with its cover and synced lyrics. */
async function verify(api: Client): Promise<void> {
	for (const album of DEMO_ALBUMS) {
		const found = await findAlbum(api, album);
		if (!found) throw new Error(`${album.title} is missing after the import`);
		const cover = await api.call(`/api/v1/library/albums/${found.id}/cover`);
		if (!cover.headers.get("content-type")?.startsWith("image/"))
			throw new Error(`${album.title} has no cover`);
		const { tracks } = await api.json<{ tracks: { id: string }[] }>(
			`/api/v1/library/albums/${found.id}`,
		);
		if (tracks.length !== album.tracks.length)
			throw new Error(`${album.title} has ${tracks.length} Tracks`);
		const { lyrics } = await api.json<{ lyrics: string }>(
			`/api/v1/library/tracks/${tracks[0].id}/lyrics`,
		);
		if (!/^\[\d+:\d+(\.\d+)?\]/.test(lyrics))
			throw new Error(`${album.title} has no synced lyrics`);
	}
}

async function main(): Promise<void> {
	const baseUrl = process.env.SEED_URL?.replace(/\/$/, "");
	if (!baseUrl) throw new Error("Set SEED_URL to the Music Server's address.");
	const api = client(
		baseUrl,
		process.env.SEED_USERNAME || "demo",
		process.env.SEED_PASSWORD || "demo",
		process.env.SEED_BASIC_AUTH ?? "",
	);
	await api.signIn();
	const missing: DemoAlbum[] = [];
	for (const album of DEMO_ALBUMS)
		if (!(await findAlbum(api, album))) missing.push(album);
	if (!missing.length) {
		console.log("Demo library already present; nothing to do.");
		return;
	}
	const dir = mkdtempSync(join(tmpdir(), "harmony-demo-"));
	try {
		const files = missing.flatMap((album) => buildAlbum(album, dir));
		await importFiles(api, files);
		await verify(api);
		console.log(
			`Seeded ${files.length} Tracks: ${missing.map((album) => `${album.artist} – ${album.title}`).join(", ")}.`,
		);
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

if (import.meta.main) {
	await main();
}
