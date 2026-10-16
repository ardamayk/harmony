// Prints a short CPU and memory snapshot of the built Music Server, idle and
// while one client streams a Track at playback speed. If a Desktop Client is
// running (started separately), its processes (app, WebKit, mpv) are sampled
// in both phases as they are; play something there to measure playback.
//
// Memory is PSS (proportional set size): pages shared between processes, such
// as WebKit's libraries, count once instead of once per process, so the Desktop
// Client's several processes add up honestly.
//
// Linux only: samples /proc. Local only: nothing is sent anywhere.
// Run with `mise run perf:snapshot` after `mise run build`.
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = join(import.meta.dirname, "..");
const ADDRESS = process.env.PERF_SERVER_ADDR ?? "127.0.0.1:8098";
const BASE_URL = `http://${ADDRESS}`;
const SECONDS = Number(process.env.PERF_SECONDS ?? 30);
const CLOCK_TICKS = 100; // USER_HZ on Linux
const PAGE_KB = 4;
const DESKTOP_APP = "earthly-audio-desktop";

type Sample = {
	cpuPercent: number;
	memoryMb: number;
	/** Memory per process name at this sample, in MB. */
	byProcess: Map<string, number>;
};

function readStat(
	pid: number,
): { ppid: number; ticks: number; comm: string } | null {
	try {
		const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
		const close = stat.lastIndexOf(")");
		const comm = stat.slice(stat.indexOf("(") + 1, close);
		const fields = stat.slice(close + 2).split(" ");
		return {
			ppid: Number(fields[1]),
			ticks: Number(fields[11]) + Number(fields[12]),
			comm,
		};
	} catch {
		return null;
	}
}

/** PSS in kB, falling back to RSS where smaps_rollup is unreadable. */
function memoryKb(pid: number): number {
	try {
		const rollup = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
		const pss = /^Pss:\s+(\d+) kB/m.exec(rollup);
		if (pss) return Number(pss[1]);
	} catch {
		// fall through to RSS
	}
	try {
		const pages = Number(
			readFileSync(`/proc/${pid}/statm`, "utf8").split(" ")[1],
		);
		return pages * PAGE_KB;
	} catch {
		return 0;
	}
}

function processTable() {
	const table = new Map<number, { ppid: number; comm: string }>();
	for (const entry of readdirSync("/proc")) {
		const pid = Number(entry);
		if (!Number.isInteger(pid)) continue;
		const stat = readStat(pid);
		if (stat) table.set(pid, { ppid: stat.ppid, comm: stat.comm });
	}
	return table;
}

/** The root and every descendant still alive. */
function processTree(root: number): number[] {
	const table = processTable();
	const tree = [root];
	for (let index = 0; index < tree.length; index += 1) {
		for (const [pid, { ppid }] of table)
			if (ppid === tree[index] && !tree.includes(pid)) tree.push(pid);
	}
	return tree.filter((pid) => table.has(pid));
}

function findDesktopClient(): number | undefined {
	for (const [pid, { comm }] of processTable())
		if (DESKTOP_APP.startsWith(comm)) return pid; // comm is cut at 15 chars
	return undefined;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Samples a process tree once a second for the phase's length. */
async function sampleTree(root: number, seconds: number): Promise<Sample[]> {
	const ticksOf = (pids: number[]) =>
		pids.reduce((sum, pid) => sum + (readStat(pid)?.ticks ?? 0), 0);
	const samples: Sample[] = [];
	let pids = processTree(root);
	let ticks = ticksOf(pids);
	let at = performance.now();
	for (let second = 0; second < seconds; second += 1) {
		await sleep(1000);
		pids = processTree(root);
		const nextTicks = ticksOf(pids);
		const now = performance.now();
		const byProcess = new Map<string, number>();
		for (const pid of pids) {
			const name = readStat(pid)?.comm ?? "?";
			byProcess.set(name, (byProcess.get(name) ?? 0) + memoryKb(pid) / 1024);
		}
		samples.push({
			cpuPercent:
				((nextTicks - ticks) / CLOCK_TICKS / ((now - at) / 1000)) * 100,
			memoryMb: [...byProcess.values()].reduce((sum, mb) => sum + mb, 0),
			byProcess,
		});
		ticks = nextTicks;
		at = now;
	}
	return samples;
}

function summary(label: string, samples: Sample[]): string {
	if (samples.length === 0) return `  ${label.padEnd(26)} not running`;
	const average = (values: number[]) =>
		values.reduce((sum, value) => sum + value, 0) / values.length;
	const cpu = samples.map((sample) => sample.cpuPercent);
	const memory = samples.map((sample) => sample.memoryMb);
	const percent = (value: number) => `${value.toFixed(1).padStart(5)}%`;
	const megabytes = (value: number) => `${value.toFixed(0).padStart(4)} MB`;
	const line = `  ${label.padEnd(26)} CPU avg ${percent(average(cpu))}  peak ${percent(Math.max(...cpu))}   RAM avg ${megabytes(average(memory))}  peak ${megabytes(Math.max(...memory))}`;
	const parts = [...(samples.at(-1)?.byProcess ?? [])]
		.sort((a, b) => b[1] - a[1])
		.map(([name, mb]) => `${name} ${mb.toFixed(0)} MB`);
	return parts.length > 1
		? `${line}\n  ${"".padEnd(26)} ${parts.join(" · ")}`
		: line;
}

async function waitForServer(server: ChildProcess) {
	for (let attempt = 0; attempt < 100; attempt += 1) {
		if (server.exitCode !== null) throw new Error("Music Server exited early.");
		try {
			const response = await fetch(`${BASE_URL}/api/v1/health`);
			if (response.ok) return;
		} catch {
			// not listening yet
		}
		await sleep(100);
	}
	throw new Error(`Music Server did not answer on ${BASE_URL}.`);
}

/** Reads one Track at its own byte rate, like a player buffering ahead. */
async function streamAtPlaybackSpeed(signal: AbortSignal) {
	const list = (await (
		await fetch(`${BASE_URL}/api/v1/library/tracks?limit=1`)
	).json()) as {
		items: Array<{ id: string; title: string; durationMs?: number }>;
	};
	const track = list.items[0];
	if (!track) return "no Tracks in the library; streaming phase skipped";
	const url = `${BASE_URL}/api/v1/tracks/${track.id}/stream`;
	const size = Number(
		(await fetch(url, { method: "HEAD" })).headers.get("content-length") ?? 0,
	);
	const seconds = Math.max(1, (track.durationMs ?? 180_000) / 1000);
	const chunk = Math.max(16_384, Math.ceil(size / seconds));
	void (async () => {
		for (let offset = 0; offset < size && !signal.aborted; offset += chunk) {
			const response = await fetch(url, {
				headers: {
					Range: `bytes=${offset}-${Math.min(size, offset + chunk) - 1}`,
				},
				signal,
			}).catch(() => null);
			await response?.arrayBuffer().catch(() => null);
			await sleep(1000);
		}
	})();
	return `"${track.title}", ${Math.round(chunk / 1024)} KB/s`;
}

const binary = process.env.EARTHLY_SERVER_BINARY ?? join(ROOT, "bin/server");
if (!existsSync(binary)) {
	console.error(`Missing ${binary}; run 'mise run build' first.`);
	process.exit(1);
}

const server = spawn(binary, [], {
	cwd: join(ROOT, "server"),
	env: { ...process.env, SERVER_ADDR: ADDRESS },
	stdio: "ignore",
});

try {
	await waitForServer(server);
	const serverPid = server.pid as number;
	const desktopPid = findDesktopClient();
	console.log(
		`Performance snapshot, ${SECONDS}s per phase (built Music Server on ${ADDRESS}${desktopPid ? `, Desktop Client pid ${desktopPid}` : ""})`,
	);

	const [idleServer, idleDesktop] = await Promise.all([
		sampleTree(serverPid, SECONDS),
		desktopPid ? sampleTree(desktopPid, SECONDS) : Promise.resolve([]),
	]);
	console.log("\nIdle");
	console.log(summary("Music Server", idleServer));
	console.log(summary("Desktop Client (as is)", idleDesktop));

	const streaming = new AbortController();
	const streamed = await streamAtPlaybackSpeed(streaming.signal);
	const [playServer, playDesktop] = await Promise.all([
		sampleTree(serverPid, SECONDS),
		desktopPid ? sampleTree(desktopPid, SECONDS) : Promise.resolve([]),
	]);
	streaming.abort();
	console.log(`\nStreaming one Track (${streamed})`);
	console.log(summary("Music Server", playServer));
	console.log(summary("Desktop Client (as is)", playDesktop));
	if (!desktopPid)
		console.log(
			"\nNo Desktop Client running; start it (mise run start) to include it.",
		);
} finally {
	server.kill("SIGTERM");
}
