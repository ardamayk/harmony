// Differential check of the Rust Music Server (server-rs/) against the Go
// one (server/) while it is ported module by module (ADR 0025). Both start
// on their own copy of the same database, receive the same requests in the
// same order, and must answer with the same status, content type and body
// (JSON compared structurally). A route the Rust server does not serve yet
// falls through to the Web Client's index.html and is reported as not
// ported, not as a difference.
//
// Requests that write are fine: each server only changes its own copy.
// Local only: nothing is sent anywhere.
// Run with `mise run server-rs:parity` after `mise run server:build` and
// `mise run server-rs:build`.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ROOT = join(import.meta.dirname, "..");
const GO_BINARY = process.env.EARTHLY_SERVER_BINARY ?? join(ROOT, "bin/server");
const RUST_BINARY =
	process.env.EARTHLY_RUST_SERVER_BINARY ??
	join(ROOT, "target/release/earthly-music-server");
const SOURCE_DB =
	process.env.EARTHLY_PARITY_SOURCE_DB ?? join(ROOT, "server/data/app.db");

type Probe = {
	method?: string;
	path: string;
	body?: unknown;
	/** A request body sent as is, for malformed JSON. */
	raw?: string;
	/**
	 * Remembers this answer's `id` under a name, separately per server, for
	 * later probes to use as {$name}: each server creates its own IDs.
	 */
	capture?: string;
	headers?: Record<string, string>;
};

type Answer = { status: number; type: string; body: unknown; isSpa: boolean };

/** Fields whose values legitimately differ between two runs. */
const VOLATILE_KEYS = new Set(["updatedAt", "createdAt", "tstamp"]);

/**
 * Stored columns that differ by design. Scaled-down covers come from
 * different JPEG encoders, so only their size and source are compared.
 */
const IMPLEMENTATION_COLUMNS: Record<string, string[]> = {
	album_cover_variants: ["data"],
};

/** "jpeg 128x128" for a JPEG or PNG, else the byte count. */
function describeImage(bytes: Uint8Array): string {
	if (bytes[0] === 0x89 && bytes[1] === 0x50) {
		const view = new DataView(bytes.buffer, bytes.byteOffset);
		return `png ${view.getUint32(16)}x${view.getUint32(20)}`;
	}
	if (bytes[0] === 0xff && bytes[1] === 0xd8) {
		let offset = 2;
		while (offset + 9 < bytes.length && bytes[offset] === 0xff) {
			const marker = bytes[offset + 1];
			const length = (bytes[offset + 2] << 8) | bytes[offset + 3];
			if (
				marker >= 0xc0 &&
				marker <= 0xcf &&
				![0xc4, 0xc8, 0xcc].includes(marker)
			) {
				const height = (bytes[offset + 5] << 8) | bytes[offset + 6];
				const width = (bytes[offset + 7] << 8) | bytes[offset + 8];
				return `jpeg ${width}x${height}`;
			}
			offset += 2 + length;
		}
	}
	return `${bytes.length} bytes`;
}

/**
 * Requests in order. Path segments in braces are filled from earlier
 * answers of the Go server, e.g. {album} with the first Album's id.
 */
/**
 * Library Search queries drawn from the library itself: whole titles,
 * single words, prefixes, typos (two letters swapped), credits, and the
 * validation edges.
 */
function searchProbes(titles: string[], names: string[]): Probe[] {
	const queries = new Set<string>([
		"a",
		"... / !",
		"x".repeat(200),
		"x".repeat(201),
		"live",
		"remix",
		"(live)",
		"zzzzqqq",
	]);
	for (const title of titles.slice(0, 25)) {
		queries.add(title);
		const words = title.split(/\s+/).filter(Boolean);
		if (words[0]) {
			queries.add(words[0]);
			queries.add(words[0].slice(0, 3));
		}
		if (words.length > 1)
			queries.add(`${words[0]} ${words.at(-1)?.slice(0, 2)}`);
		const long = words.find((word) => word.length >= 5);
		if (long) {
			queries.add(
				long.slice(0, 1) + long.slice(2, 3) + long.slice(1, 2) + long.slice(3),
			);
			queries.add(long.slice(0, -1));
		}
	}
	for (const name of names.slice(0, 15)) {
		queries.add(name);
		queries.add(name.toUpperCase());
		queries.add(name.split(/\s+/)[0] ?? name);
	}
	const probes: Probe[] = [...queries].map((query) => ({
		path: `/api/v1/library/search?q=${encodeURIComponent(query)}`,
	}));
	probes.push(
		{ path: "/api/v1/library/search" },
		{ path: "/api/v1/library/search?q=%20%20" },
		{
			path: `/api/v1/library/search?q=${encodeURIComponent(titles[0] ?? "a")}&all=true`,
		},
		{ path: "/api/v1/library/search?q=a&all=yes" },
		{ path: "/api/v1/library/search?q=a&all=true&all=false" },
	);
	return probes;
}

function unknownPlaceholder(name: string): never {
	throw new Error(`Probe uses {${name}}, which nothing defines or captured.`);
}

function corpus(ids: Record<string, string>): Probe[] {
	const fill = (path: string) =>
		path.replace(
			/\{(\w+)\}/g,
			(_, key: string) => ids[key] ?? unknownPlaceholder(key),
		);
	const fillBody = (body: unknown) =>
		body === undefined ? undefined : JSON.parse(fill(JSON.stringify(body)));
	const probes: Probe[] = [
		{ path: "/api/v1/health" },
		{ path: "/api/v1/me" },
		{ path: "/api/openapi.yaml" },
		{ path: "/docs" },
		{ path: "/api/v1/preferences" },
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			body: { theme: { mode: "dark" }, playback: { showWaveform: false } },
		},
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			body: { layout: { sizes: [60, 0, 70], collapsed: { left: true } } },
		},
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			body: { layout: { panels: { left: [], right: null } } },
		},
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			body: { playback: { playbackRate: 3 } },
		},
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			body: { playback: { seekStepSeconds: 2.5 } },
		},
		{ method: "PATCH", path: "/api/v1/preferences", body: "not an object" },
		{
			method: "PATCH",
			path: "/api/v1/preferences",
			raw: '{"theme":{}} trailing',
		},
		{ method: "PATCH", path: "/api/v1/preferences", raw: "" },
		{ path: "/api/v1/preferences" },
		{ path: "/api/v1/library/artists?limit=50" },
		{ path: "/api/v1/library/artists?q={artistQuery}" },
		{ path: "/api/v1/library/artists?limit=2&offset=1" },
		{ path: "/api/v1/library/artists?limit=0&offset=-4" },
		{ path: "/api/v1/library/albums?limit=50" },
		{ path: "/api/v1/library/albums?artistId={artist}" },
		{ path: "/api/v1/library/albums?q={albumQuery}" },
		{ path: "/api/v1/library/albums?limit=3&offset=2" },
		{ path: "/api/v1/library/albums/{album}" },
		{ path: "/api/v1/library/albums/does-not-exist" },
		{ path: "/api/v1/library/albums/{album}/cover" },
		{ path: "/api/v1/library/albums/{album}/cover?size=64" },
		{ path: "/api/v1/library/albums/{album}/cover?size=128" },
		{ path: "/api/v1/library/albums/{album}/cover?size=256" },
		{ path: "/api/v1/library/albums/{album}/cover?size=512" },
		{ path: "/api/v1/library/albums/{album}/cover?size=1024" },
		{ path: "/api/v1/library/albums/{album}/cover?size=63" },
		{ path: "/api/v1/library/albums/does-not-exist/cover" },
		{ path: "/api/v1/library/tracks?limit=100" },
		{ path: "/api/v1/library/tracks?limit=500" },
		{ path: "/api/v1/library/tracks?artistId={artist}" },
		{ path: "/api/v1/library/tracks?q={query}" },
		{ path: "/api/v1/library/tracks?q={genreQuery}" },
		{ path: "/api/v1/library/tracks/{track}" },
		{ path: "/api/v1/library/tracks/does-not-exist" },
		{ path: "/api/v1/library/tracks/{track}/lyrics" },
		{ path: "/api/v1/library/tracks/does-not-exist/lyrics" },
		{ path: "/api/v1/library/tracks/{track}/waveform" },
		{ path: "/api/v1/library/tracks/does-not-exist/waveform" },
		{ path: "/api/v1/library/search?q={query}" },
		{ path: "/api/v1/playback/queue" },
		{ path: "/api/v1/playlists" },
		{
			method: "POST",
			path: "/api/v1/playlists",
			body: { name: " Parity Mix " },
			capture: "mix",
		},
		{ method: "POST", path: "/api/v1/playlists", body: { name: "   " } },
		{ method: "POST", path: "/api/v1/playlists", body: { name: "Parity Mix" } },
		{ method: "POST", path: "/api/v1/playlists", raw: "nope" },
		{ path: "/api/v1/playlists/{$mix}" },
		{
			method: "POST",
			path: "/api/v1/playlists/{$mix}/tracks",
			body: { trackId: "{track}" },
		},
		{
			method: "POST",
			path: "/api/v1/playlists/{$mix}/tracks",
			body: { trackId: "{track}" },
		},
		{
			method: "POST",
			path: "/api/v1/playlists/{$mix}/tracks",
			body: { trackId: "{secondTrack}" },
		},
		{
			method: "POST",
			path: "/api/v1/playlists/{$mix}/tracks",
			body: { trackId: "" },
		},
		{
			method: "POST",
			path: "/api/v1/playlists/{$mix}/tracks",
			body: { trackId: "missing" },
		},
		{
			method: "POST",
			path: "/api/v1/playlists/missing/tracks",
			body: { trackId: "{track}" },
		},
		{ path: "/api/v1/playlists" },
		{ method: "DELETE", path: "/api/v1/playlists/{$mix}/tracks/{track}" },
		{ method: "DELETE", path: "/api/v1/playlists/{$mix}/tracks/{secondTrack}" },
		{ path: "/api/v1/playlists/{$mix}" },
		{
			method: "POST",
			path: "/api/v1/playlists/{favorites}/tracks",
			body: { trackId: "{track}" },
		},
		{ method: "DELETE", path: "/api/v1/playlists/{favorites}/tracks/{track}" },
		{ path: "/api/v1/playlists/{favorites}" },
		{ path: "/api/v1/playlists/missing" },
		{ path: "/api/v1/radio/stations" },
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: {
				name: " Parity FM ",
				streamUrl: " https://radio.example/parity ",
				tags: ["talk"],
				bitrate: 96,
			},
			capture: "station",
		},
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: { streamUrl: "https://radio.example/x" },
		},
		{ method: "POST", path: "/api/v1/radio/stations", body: { name: "x" } },
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: { name: "x", streamUrl: "http://192.168.1.4/s" },
		},
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: { name: "x", streamUrl: "ftp://radio.example/s" },
		},
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: {
				name: "x",
				streamUrl: "https://radio.example/s",
				homepageUrl: "radio.example",
			},
		},
		{
			method: "POST",
			path: "/api/v1/radio/stations",
			body: { name: "x", streamUrl: "https://radio.example/s", bitrate: 1.5 },
		},
		{ method: "POST", path: "/api/v1/radio/stations", raw: "[" },
		{ path: "/api/v1/radio/stations/{$station}" },
		{ path: "/api/v1/radio/stations/missing" },
		{
			method: "PATCH",
			path: "/api/v1/radio/stations/{$station}",
			body: {
				name: " Renamed ",
				isFavorite: true,
				tags: ["a", "b"],
				position: 7,
			},
		},
		{
			method: "PATCH",
			path: "/api/v1/radio/stations/{$station}",
			body: { tags: null, codec: " AAC " },
		},
		{
			method: "PATCH",
			path: "/api/v1/radio/stations/{$station}",
			body: { streamUrl: "http://localhost/s" },
		},
		{
			method: "PATCH",
			path: "/api/v1/radio/stations/missing",
			body: { name: "x" },
		},
		{ path: "/api/v1/radio/stations/{$station}/now-playing" },
		{ path: "/api/v1/radio/stations/preview:u-1/now-playing" },
		{ path: "/api/v1/radio/stations/missing/now-playing" },
		{
			path: "/api/v1/radio/search?q=%20jazz%20&limit=5&offset=2&country=Germany&minBitrate=128",
		},
		{
			path: "/api/v1/radio/search?q=a+b&tag=r%26b&language=german&limit=abc&offset=-1",
		},
		{ path: "/api/v1/radio/search?codec=aac&limit=3&offset=1" },
		{ path: "/api/v1/radio/search?codecGroup=AAC&limit=2&offset=5" },
		{ path: "/api/v1/radio/catalog/countries" },
		{ path: "/api/v1/radio/catalog/tags" },
		{
			method: "POST",
			path: "/api/v1/radio/import",
			body: { stationUuid: "u-1" },
			capture: "imported",
		},
		{
			method: "POST",
			path: "/api/v1/radio/import",
			body: { stationUuid: "u-1" },
		},
		{ method: "POST", path: "/api/v1/radio/import", body: {} },
		{
			method: "POST",
			path: "/api/v1/radio/import",
			body: { stationUuid: "unknown" },
		},
		{
			method: "POST",
			path: "/api/v1/radio/import",
			body: { stationUuid: "broken" },
		},
		{
			method: "POST",
			path: "/api/v1/radio/import",
			body: {
				stationUuid: "unknown",
				result: {
					stationUuid: "manual-1",
					name: "Sent Along",
					streamUrl: "https://radio.example/sent",
				},
			},
			capture: "sent",
		},
		{ path: "/api/v1/radio/stations" },
		{ path: "/api/v1/radio/preview/u-private/stream" },
		{ path: "/api/v1/radio/preview/unknown/stream" },
		{
			path: "/api/v1/radio/stations/{$station}/stream?url=https%3A%2F%2Fradio.example%2Fx",
		},
		{ path: "/api/v1/radio/stations/{$station}/stream?resource=forged.token" },
		{ path: "/api/v1/radio/stations/missing/stream" },
		{
			method: "POST",
			path: "/api/v1/radio/preview",
			body: { stationUuid: "u-private" },
		},
		{ method: "POST", path: "/api/v1/radio/preview", body: {} },
		{ method: "DELETE", path: "/api/v1/radio/stations/{$station}" },
		{ method: "DELETE", path: "/api/v1/radio/stations/{$station}" },
		{ path: "/api/v1/import-history" },
	];
	return probes.map((probe) => ({
		...probe,
		path: fill(probe.path),
		body: fillBody(probe.body),
	}));
}

/**
 * Every table's rows as sorted JSON lines, timestamps left out, so the two
 * copies can be compared after both servers wrote to them.
 */
function dumpDatabase(path: string): Map<string, string[]> {
	const script = `
import json, sqlite3, sys
connection = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
tables = [name for (name,) in connection.execute(
    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")]
dump = {}
for table in tables:
    cursor = connection.execute(f'SELECT * FROM "{table}"')
    columns = [column[0] for column in cursor.description]
    skipped = json.loads(sys.argv[2]).get(table, [])
    keep = [i for i, name in enumerate(columns)
            if not (name.endswith("_at") or name in skipped
                    or name in ("tstamp", "id") and table == "goose_db_version")]
    dump[table] = sorted(
        json.dumps([row[i].hex() if isinstance(row[i], bytes) else row[i] for i in keep])
        for row in cursor)
print(json.dumps(dump))
`;
	const output = execFileSync(
		"python3",
		["-c", script, path, JSON.stringify(IMPLEMENTATION_COLUMNS)],
		{
			maxBuffer: 1 << 30,
		},
	);
	return new Map(
		Object.entries(JSON.parse(output.toString()) as Record<string, string[]>),
	);
}

/**
 * A stand-in for Radio Browser on loopback, so neither server reaches the
 * real catalog. Search echoes the query it received as the station name,
 * which shows whether both servers ask the catalog the same thing.
 */
function startFakeRadioBrowser(port: number) {
	const station = (
		uuid: string,
		name: string,
		extra: Record<string, unknown> = {},
	) => ({
		stationuuid: uuid,
		name,
		url: "https://radio.example/live",
		url_resolved: "",
		homepage: "https://radio.example",
		favicon: "",
		country: "Germany",
		language: "german",
		tags: "jazz, smooth,,",
		codec: "MP3",
		bitrate: 128.7,
		votes: "42",
		lastcheckok: 1,
		lastchecktime_iso8601: "2026-10-01T10:00:00Z",
		lastcheckoktime_iso8601: " ",
		...extra,
	});
	const server = createServer((request, response) => {
		const url = new URL(request.url ?? "/", "http://fake");
		const send = (body: unknown, status = 200) => {
			response.writeHead(status, { "content-type": "application/json" });
			response.end(JSON.stringify(body));
		};
		if (url.pathname === "/json/stations/search") {
			const codec = url.searchParams.get("codec");
			if (codec === "AAC" || codec === "AAC+") {
				return send([
					station(`${codec}-b`, `beta ${codec}`, { codec }),
					station(`${codec}-a`, `Alpha ${codec}`, {
						codec,
						lastcheckok: false,
					}),
				]);
			}
			return send([
				station("echo", `search ${url.search}`, { lastcheckok: "true" }),
			]);
		}
		if (url.pathname.startsWith("/json/stations/byuuid/")) {
			const uuid = decodeURIComponent(
				url.pathname.slice("/json/stations/byuuid/".length),
			);
			if (uuid === "broken") return send({ error: "down" }, 500);
			if (uuid === "u-1") return send([station("u-1", " Jazz One ")]);
			if (uuid === "u-private") {
				return send([
					station("u-private", "Private", { url: "http://10.0.0.1/live" }),
				]);
			}
			return send([]);
		}
		if (url.pathname === "/json/countries") {
			return send([
				{ name: "Germany", iso_3166_1: "de", stationcount: 120 },
				{ name: "Türkiye", iso_3166_1: "tr", stationcount: "33" },
			]);
		}
		if (url.pathname === "/json/tags") {
			return send([{ name: "jazz", stationcount: 7.9 }, { name: "rock" }]);
		}
		send([], 404);
	});
	server.listen(port, "127.0.0.1");
	return server;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function start(binary: string, port: number, database: string) {
	const child = spawn(binary, [], {
		cwd: join(ROOT, "server"),
		env: {
			...process.env,
			SERVER_ADDR: `127.0.0.1:${port}`,
			DATABASE_PATH: database,
			RADIO_BROWSER_BASE_URL: "http://127.0.0.1:8194",
			RUST_LOG: "warn",
		},
		stdio: "ignore",
	});
	const base = `http://127.0.0.1:${port}`;
	for (let attempt = 0; attempt < 200; attempt += 1) {
		if (child.exitCode !== null) throw new Error(`${binary} exited early`);
		const healthy = await fetch(`${base}/api/v1/health`)
			.then((response) => response.ok)
			.catch(() => false);
		if (healthy) return { child, base };
		await sleep(50);
	}
	throw new Error(`${binary} did not answer on ${base}`);
}

async function ask(base: string, probe: Probe): Promise<Answer> {
	const response = await fetch(`${base}${probe.path}`, {
		method: probe.method ?? "GET",
		redirect: "manual",
		headers: {
			...(probe.body === undefined
				? {}
				: { "content-type": "application/json" }),
			...probe.headers,
		},
		body:
			probe.raw ??
			(probe.body === undefined ? undefined : JSON.stringify(probe.body)),
	});
	const type = (response.headers.get("content-type") ?? "").split(";")[0];
	const bytes = new Uint8Array(await response.arrayBuffer());
	const text = new TextDecoder().decode(bytes);
	let body: unknown = text;
	if (type === "application/json") body = JSON.parse(text || "null");
	else if (type.startsWith("image/")) body = describeImage(bytes);
	else if (!type.startsWith("text/") && type !== "application/yaml")
		body = `${bytes.length} bytes`;
	const isSpa = type === "text/html" && text.includes('<div id="root"');
	return { status: response.status, type, body, isSpa };
}

function normalize(value: unknown): unknown {
	if (Array.isArray(value)) return value.map(normalize);
	if (value && typeof value === "object") {
		return Object.fromEntries(
			Object.entries(value)
				.filter(([key]) => !VOLATILE_KEYS.has(key))
				.sort(([a], [b]) => a.localeCompare(b))
				.map(([key, inner]) => [key, normalize(inner)]),
		);
	}
	return value;
}

function differences(go: Answer, rust: Answer): string[] {
	const found: string[] = [];
	if (go.status !== rust.status)
		found.push(`status ${go.status} ≠ ${rust.status}`);
	if (go.type !== rust.type) found.push(`type ${go.type} ≠ ${rust.type}`);
	if (go.type === "application/yaml" && rust.type === "application/yaml") {
		return found; // Same contract; Go re-serializes it with sorted keys.
	}
	const left = JSON.stringify(normalize(go.body));
	const right = JSON.stringify(normalize(rust.body));
	if (left !== right) {
		found.push(
			`body\n      go   ${left.slice(0, 300)}\n      rust ${right.slice(0, 300)}`,
		);
	}
	return found;
}

for (const binary of [GO_BINARY, RUST_BINARY]) {
	if (!existsSync(binary)) {
		console.error(`Missing ${binary}; build both servers first.`);
		process.exit(1);
	}
}

const workspace = mkdtempSync(join(tmpdir(), "earthly-parity-"));
const fakeRadioBrowser = startFakeRadioBrowser(8194);
const children: ChildProcess[] = [];
try {
	const goDb = join(workspace, "go.db");
	const rustDb = join(workspace, "rust.db");
	// A WAL database is consistent only with its -wal file; copy both.
	for (const suffix of ["", "-wal"]) {
		if (existsSync(SOURCE_DB + suffix)) {
			copyFileSync(SOURCE_DB + suffix, goDb + suffix);
			copyFileSync(SOURCE_DB + suffix, rustDb + suffix);
		}
	}
	const go = await start(GO_BINARY, 8191, goDb);
	const rust = await start(RUST_BINARY, 8192, rustDb);
	children.push(go.child, rust.child);

	const first = async <T>(
		path: string,
		pick: (body: T) => string | undefined,
	) => pick((await (await fetch(`${go.base}${path}`)).json()) as T) ?? "";
	type Items = { items: Array<{ id: string; title?: string }> };
	type Albums = {
		items: Array<{
			id: string;
			title: string;
			artistId: string;
			artistName: string;
			genres?: string[];
		}>;
	};
	const albumList = (await (
		await fetch(`${go.base}/api/v1/library/albums?limit=500`)
	).json()) as Albums;
	const albumWithGenre = albumList.items.find((album) => album.genres?.length);
	const tracks = (await (
		await fetch(`${go.base}/api/v1/library/tracks?limit=500`)
	).json()) as Items;
	const ids: Record<string, string> = {
		album: albumList.items[0]?.id ?? "",
		artist: albumList.items[0]?.artistId ?? "",
		artistQuery: encodeURIComponent(
			albumList.items[0]?.artistName.slice(0, 3) ?? "",
		),
		albumQuery: encodeURIComponent(
			albumList.items[1]?.title.split(/\s+/)[0] ?? "",
		),
		genreQuery: encodeURIComponent(albumWithGenre?.genres?.[0] ?? ""),
		track: tracks.items[0]?.id ?? "",
		secondTrack: tracks.items[1]?.id ?? "",
		query: encodeURIComponent(tracks.items[0]?.title?.split(/\s+/)[0] ?? ""),
		favorites: await first<{
			items: Array<{ id: string; isDefault: boolean }>;
		}>(
			"/api/v1/playlists",
			(b) => b.items.find((playlist) => playlist.isDefault)?.id,
		),
	};
	for (const [name, value] of Object.entries(ids)) {
		if (!value) throw new Error(`The library has nothing for {${name}}.`);
	}

	const goIds = new Map<string, string>();
	const rustIds = new Map<string, string>();
	const withIds = (probe: Probe, captured: Map<string, string>): Probe => {
		const fill = (text: string) =>
			text.replace(
				/\{\$(\w+)\}/g,
				(_, key: string) => captured.get(key) ?? unknownPlaceholder(`$${key}`),
			);
		return {
			...probe,
			path: fill(probe.path),
			body:
				probe.body === undefined
					? undefined
					: JSON.parse(fill(JSON.stringify(probe.body))),
		};
	};
	/** Rust's own IDs replaced by the Go server's for the same probes. */
	const asGoIds = (text: string) => {
		for (const [name, rustId] of rustIds) {
			const goId = goIds.get(name);
			if (goId) text = text.split(rustId).join(goId);
		}
		return text;
	};

	let matched = 0;
	let differing = 0;
	let notPorted = 0;
	const trackTitles = tracks.items.map((track) => track.title ?? "");
	const names = [
		...new Set(
			albumList.items.flatMap((album) => [album.artistName, album.title]),
		),
	];
	for (const probe of [...corpus(ids), ...searchProbes(trackTitles, names)]) {
		const label = `${probe.method ?? "GET"} ${probe.path}`;
		const goAnswer = await ask(go.base, withIds(probe, goIds));
		const rustAnswer = await ask(rust.base, withIds(probe, rustIds));
		if (probe.capture) {
			const idOf = (answer: Answer) =>
				(answer.body as { id?: string } | null)?.id;
			const goId = idOf(goAnswer);
			const rustId = idOf(rustAnswer);
			if (goId) goIds.set(probe.capture, goId);
			if (rustId) rustIds.set(probe.capture, rustId);
		}
		rustAnswer.body = JSON.parse(asGoIds(JSON.stringify(rustAnswer.body)));
		if (rustAnswer.isSpa && !goAnswer.isSpa) {
			notPorted += 1;
			console.log(`  ·  ${label}  (not ported)`);
			continue;
		}
		const found = differences(goAnswer, rustAnswer);
		if (found.length === 0) {
			matched += 1;
			console.log(`  ✓  ${label}`);
		} else {
			differing += 1;
			console.log(`  ✗  ${label}\n      ${found.join("\n      ")}`);
		}
	}
	console.log(
		`\n${matched} match, ${differing} differ, ${notPorted} not ported yet`,
	);

	// Stop both servers so their WAL is checkpointed, then compare what
	// they stored.
	for (const child of children) child.kill("SIGTERM");
	await sleep(500);
	const goTables = dumpDatabase(goDb);
	const rustTables = new Map(
		[...dumpDatabase(rustDb)].map(([table, rows]) => [
			table,
			rows.map(asGoIds).sort(),
		]),
	);
	const differingTables: string[] = [];
	for (const [table, goRows] of goTables) {
		const rustRows = rustTables.get(table) ?? [];
		if (JSON.stringify(goRows) !== JSON.stringify(rustRows)) {
			const onlyGo = goRows.filter((row) => !rustRows.includes(row));
			const onlyRust = rustRows.filter((row) => !goRows.includes(row));
			differingTables.push(
				`  ✗  table ${table}\n      go   ${onlyGo
					.slice(0, 3)
					.map((row) => row.slice(0, 200))
					.join("\n           ")}\n      rust ${onlyRust
					.slice(0, 3)
					.map((row) => row.slice(0, 200))
					.join("\n           ")}`,
			);
		}
	}
	console.log(
		differingTables.length === 0
			? `stored data: all ${goTables.size} tables match`
			: `stored data:\n${differingTables.join("\n")}`,
	);
	process.exitCode = differing > 0 || differingTables.length > 0 ? 1 : 0;
} finally {
	for (const child of children) child.kill("SIGTERM");
	fakeRadioBrowser.close();
	await sleep(200);
	rmSync(workspace, { recursive: true, force: true });
}
