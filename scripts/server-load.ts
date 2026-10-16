// Load snapshot of a built Music Server: startup time, idle footprint,
// latency and throughput of the read-only endpoints the clients hit most,
// peak memory, CPU and disk I/O under load, and unthrottled stream
// throughput. It only reads
// (album cover variants may be cached on first request), so it can run
// against a real library. Compare runs before and after server changes, or
// across server implementations (EARTHLY_SERVER_BINARY).
//
// Linux only (samples /proc). Local only: nothing is sent anywhere.
// Run with `mise run perf:server-load` after `mise run server:build`.
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = join(import.meta.dirname, "..");
const ADDRESS = process.env.PERF_SERVER_ADDR ?? "127.0.0.1:8097";
const BASE_URL = `http://${ADDRESS}`;
const REQUESTS = Number(process.env.PERF_REQUESTS ?? 1000);
const CONCURRENCY = Number(process.env.PERF_CONCURRENCY ?? 8);
const IDLE_SECONDS = Number(process.env.PERF_IDLE_SECONDS ?? 15);
const CLOCK_TICKS = 100; // USER_HZ on Linux

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function cpuTicks(pid: number): number {
	const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
	const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
	return Number(fields[11]) + Number(fields[12]);
}

/** PSS in MB (one process: same as private + its share of libraries). */
function memoryMb(pid: number): number {
	const rollup = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
	return Number(/^Pss:\s+(\d+) kB/m.exec(rollup)?.[1] ?? 0) / 1024;
}

/** Bytes the process made the storage layer read and write (page cache hits excluded). */
function diskBytes(pid: number): { read: number; write: number } {
	const io = readFileSync(`/proc/${pid}/io`, "utf8");
	const field = (name: string) =>
		Number(new RegExp(`^${name}: (\\d+)`, "m").exec(io)?.[1] ?? 0);
	return { read: field("read_bytes"), write: field("write_bytes") };
}

const mb = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);

/** Samples CPU, memory and disk I/O every 250ms while `work` runs. */
async function sampleWhile<T>(pid: number, work: () => Promise<T>) {
	let peakMb = memoryMb(pid);
	let running = true;
	const startTicks = cpuTicks(pid);
	const startDisk = diskBytes(pid);
	const started = performance.now();
	const sampler = (async () => {
		while (running) {
			peakMb = Math.max(peakMb, memoryMb(pid));
			await sleep(250);
		}
	})();
	const result = await work();
	running = false;
	await sampler;
	const seconds = (performance.now() - started) / 1000;
	const cpuPercent =
		((cpuTicks(pid) - startTicks) / CLOCK_TICKS / seconds) * 100;
	const endDisk = diskBytes(pid);
	const disk = {
		read: endDisk.read - startDisk.read,
		write: endDisk.write - startDisk.write,
	};
	return { result, peakMb, cpuPercent, seconds, disk };
}

function percentile(sorted: number[], p: number): number {
	return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
}

async function load(path: string) {
	const latencies: number[] = [];
	let failures = 0;
	let next = 0;
	const worker = async () => {
		while (next < REQUESTS) {
			next += 1;
			const started = performance.now();
			try {
				const response = await fetch(`${BASE_URL}${path}`);
				await response.arrayBuffer();
				if (!response.ok) failures += 1;
			} catch {
				failures += 1;
			}
			latencies.push(performance.now() - started);
		}
	};
	const started = performance.now();
	await Promise.all(Array.from({ length: CONCURRENCY }, worker));
	const seconds = (performance.now() - started) / 1000;
	latencies.sort((a, b) => a - b);
	return {
		p50: percentile(latencies, 0.5),
		p90: percentile(latencies, 0.9),
		p95: percentile(latencies, 0.95),
		p99: percentile(latencies, 0.99),
		rps: latencies.length / seconds,
		failures,
	};
}

async function json<T>(path: string): Promise<T> {
	const response = await fetch(`${BASE_URL}${path}`);
	if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
	return (await response.json()) as T;
}

const binary = process.env.EARTHLY_SERVER_BINARY ?? join(ROOT, "bin/server");
if (!existsSync(binary)) {
	console.error(`Missing ${binary}; run 'mise run server:build' first.`);
	process.exit(1);
}

const spawnedAt = performance.now();
const server = spawn(binary, [], {
	cwd: join(ROOT, "server"),
	env: { ...process.env, SERVER_ADDR: ADDRESS },
	stdio: "ignore",
});
const pid = server.pid as number;

try {
	let startupMs = 0;
	for (let attempt = 0; attempt < 400; attempt += 1) {
		if (server.exitCode !== null) throw new Error("Music Server exited early.");
		const healthy = await fetch(`${BASE_URL}/api/v1/health`)
			.then((response) => response.ok)
			.catch(() => false);
		if (healthy) {
			startupMs = performance.now() - spawnedAt;
			break;
		}
		await sleep(25);
	}
	if (!startupMs)
		throw new Error(`Music Server did not answer on ${BASE_URL}.`);

	const idle = await sampleWhile(pid, () => sleep(IDLE_SECONDS * 1000));
	console.log(`Music Server load snapshot (${binary})`);
	console.log(
		`  startup ${startupMs.toFixed(0)} ms · idle ${idle.peakMb.toFixed(0)} MB, CPU ${idle.cpuPercent.toFixed(1)}%, disk read ${mb(idle.disk.read)} MB / write ${mb(idle.disk.write)} MB over ${IDLE_SECONDS}s`,
	);

	const albums = await json<{ items: Array<{ id: string }> }>(
		"/api/v1/library/albums?limit=50",
	);
	const tracks = await json<{
		items: Array<{ id: string; title: string }>;
	}>("/api/v1/library/tracks?limit=50");
	const album = albums.items[0]?.id;
	const track = tracks.items[0];
	if (!album || !track) throw new Error("The library has no Albums or Tracks.");
	const query = encodeURIComponent(track.title.split(/\s+/)[0] ?? "a");

	const endpoints: Array<[string, string]> = [
		["health", "/api/v1/health"],
		["albums list", "/api/v1/library/albums?limit=50"],
		["album detail", `/api/v1/library/albums/${album}`],
		["tracks list (100)", "/api/v1/library/tracks?limit=100"],
		["artists list", "/api/v1/library/artists?limit=50"],
		["search", `/api/v1/library/search?q=${query}`],
		["cover 64", `/api/v1/library/albums/${album}/cover?size=64`],
		["cover 512", `/api/v1/library/albums/${album}/cover?size=512`],
		["waveform", `/api/v1/library/tracks/${track.id}/waveform`],
		["queue", "/api/v1/playback/queue"],
		["playlists", "/api/v1/playlists"],
		["radio stations", "/api/v1/radio/stations"],
	];

	console.log(
		`\n  ${REQUESTS} requests per endpoint, ${CONCURRENCY} concurrent`,
	);
	console.log(
		`  ${"endpoint".padEnd(20)}${["p50", "p90", "p95", "p99", "req/s"].map((h) => h.padStart(9)).join("")}  errors`,
	);
	const underLoad = await sampleWhile(pid, async () => {
		for (const [label, path] of endpoints) {
			// One warm-up request so cover variants and caches exist.
			await fetch(`${BASE_URL}${path}`).then((r) => r.arrayBuffer());
			const { p50, p90, p95, p99, rps, failures } = await load(path);
			const latency = [p50, p90, p95, p99]
				.map((ms) => `${ms.toFixed(1)}ms`.padStart(9))
				.join("");
			console.log(
				`  ${label.padEnd(20)}${latency}${rps.toFixed(0).padStart(9)}  ${failures} (${((failures / REQUESTS) * 100).toFixed(1)}%)`,
			);
		}
	});
	console.log(
		`  under load: peak ${underLoad.peakMb.toFixed(0)} MB, CPU ${underLoad.cpuPercent.toFixed(0)}% (of one core), disk read ${mb(underLoad.disk.read)} MB / write ${mb(underLoad.disk.write)} MB over ${underLoad.seconds.toFixed(1)}s`,
	);

	const stream = await sampleWhile(pid, async () => {
		const response = await fetch(
			`${BASE_URL}/api/v1/tracks/${track.id}/stream`,
		);
		return (await response.arrayBuffer()).byteLength;
	});
	const megabytes = stream.result / 1024 / 1024;
	console.log(
		`  stream "${track.title}": ${megabytes.toFixed(1)} MB in ${(stream.seconds * 1000).toFixed(0)} ms (${(megabytes / stream.seconds).toFixed(0)} MB/s), peak ${stream.peakMb.toFixed(0)} MB, disk read ${mb(stream.disk.read)} MB`,
	);
} finally {
	server.kill("SIGTERM");
}
