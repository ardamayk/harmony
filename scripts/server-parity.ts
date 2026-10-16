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
function corpus(ids: Record<string, string>): Probe[] {
	const fill = (path: string) =>
		path.replace(/\{(\w+)\}/g, (_, key: string) => ids[key] ?? "missing");
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
		{ path: "/api/v1/radio/stations" },
		{ path: "/api/v1/import-history" },
	];
	return probes.map((probe) => ({ ...probe, path: fill(probe.path) }));
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

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function start(binary: string, port: number, database: string) {
	const child = spawn(binary, [], {
		cwd: join(ROOT, "server"),
		env: {
			...process.env,
			SERVER_ADDR: `127.0.0.1:${port}`,
			DATABASE_PATH: database,
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
	const ids = {
		album: await first<Items>(
			"/api/v1/library/albums?limit=1",
			(b) => b.items[0]?.id,
		),
		track: await first<Items>(
			"/api/v1/library/tracks?limit=1",
			(b) => b.items[0]?.id,
		),
		query: encodeURIComponent(
			await first<Items>(
				"/api/v1/library/tracks?limit=1",
				(b) => b.items[0]?.title?.split(/\s+/)[0],
			),
		),
	};

	let matched = 0;
	let differing = 0;
	let notPorted = 0;
	for (const probe of corpus(ids)) {
		const label = `${probe.method ?? "GET"} ${probe.path}`;
		const goAnswer = await ask(go.base, probe);
		const rustAnswer = await ask(rust.base, probe);
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
	const rustTables = dumpDatabase(rustDb);
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
	await sleep(200);
	rmSync(workspace, { recursive: true, force: true });
}
