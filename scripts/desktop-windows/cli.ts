// Builds the Windows Desktop Client from WSL and runs it on the Windows side;
// in WSL the desktop:* tasks reach it through scripts/desktop.sh.
//   build  Web Client (unless --web-built), shell bundle, release core
//          cross-built with mingw-w64, pinned mpv; electron-builder's `dir`
//          target (NSIS with --installer), copied to the Windows output folder
//          (%LOCALAPPDATA%\EarthlyAudio by default)
//   start  the built client; --wait waits until it exits
//   dev    the built client with a fresh shell bundle and debug core,
//          rendering the Vite dev server in WSL with HMR
//   clean  the staging folder and the Windows output folder
//
// The Music Server is not started: point the connection screen at a running
// one (WSL forwards its http://127.0.0.1:8090 to Windows).

import {
	type ChildProcess,
	execFileSync,
	spawn,
	spawnSync,
} from "node:child_process";
import {
	copyFileSync,
	cpSync,
	existsSync,
	mkdirSync,
	renameSync,
	rmSync,
} from "node:fs";
import { join } from "node:path";
import {
	cacheDirectory,
	mpvRelease,
	repoRoot,
	stagedCoreDirectory,
	stagingDirectory,
	windowsOutputDirectory,
} from "./config.ts";
import { buildCore } from "./core.ts";
import { buildShell, buildWeb } from "./frontend.ts";
import { stageMpv } from "./mpv.ts";

const executableName = "Harmony.exe";
const coreName = "earthly-audio-core.exe";
// IPv4 on purpose: Windows resolves `localhost` to ::1 first, and WSL2
// forwards only the IPv4 loopback to the Vite server in WSL.
const devServerUrl = "http://127.0.0.1:3000";
const devServerStartupAttempts = 300;

function run(command: string, args: string[]) {
	execFileSync(command, args, { cwd: repoRoot, stdio: "inherit" });
}

/** Windows refuses to move or replace files of a running program. */
function isLocked(error: unknown): boolean {
	const code = (error as NodeJS.ErrnoException).code;
	return code === "EACCES" || code === "EBUSY" || code === "EPERM";
}

function lockedError(directory: string): Error {
	return new Error(
		`${directory} is in use; quit Harmony (tray, Quit) and try again.`,
	);
}

/**
 * Removes a Windows folder. It is renamed first so a running client fails
 * the whole step up front instead of leaving a half-deleted install.
 */
function removeWindowsDirectory(directory: string) {
	if (!existsSync(directory)) return;
	const doomed = `${directory}.old-${process.pid}`;
	try {
		renameSync(directory, doomed);
	} catch (error) {
		if (isLocked(error)) throw lockedError(directory);
		throw error;
	}
	rmSync(doomed, { recursive: true, force: true });
}

/** Overwrites files inside an installed client, which a running one locks. */
function replaceInstalled(output: string, update: () => void) {
	try {
		update();
	} catch (error) {
		if (isLocked(error)) throw lockedError(output);
		throw error;
	}
}

function installedExecutable(): string {
	const executable = join(windowsOutputDirectory(), executableName);
	if (!existsSync(executable)) {
		throw new Error(
			`Missing ${executable}; run 'mise run desktop:build' first.`,
		);
	}
	return executable;
}

async function build(options: Set<string>) {
	if (!options.has("--web-built")) buildWeb();
	buildShell();
	const core = buildCore("release");
	rmSync(stagedCoreDirectory, { recursive: true, force: true });
	mkdirSync(stagedCoreDirectory, { recursive: true });
	copyFileSync(core, join(stagedCoreDirectory, coreName));
	await stageMpv(stagedCoreDirectory, mpvRelease, cacheDirectory);

	const packaged = join(stagingDirectory, "out/win-unpacked");
	rmSync(join(stagingDirectory, "out"), { recursive: true, force: true });
	run("pnpm", [
		"--dir",
		"desktop",
		"exec",
		"electron-builder",
		"--win",
		options.has("--installer") ? "nsis" : "dir",
		"--x64",
		"--config",
		"electron-builder.yml",
		"--publish",
		"never",
	]);
	// The shell bundles everything it imports; packed node_modules would mean
	// a runtime dependency slipped into desktop/package.json.
	if (existsSync(join(packaged, "resources/app/node_modules"))) {
		throw new Error(
			"electron-builder packed node_modules; the shell must have no runtime dependencies.",
		);
	}

	const output = windowsOutputDirectory();
	removeWindowsDirectory(output);
	cpSync(packaged, output, { recursive: true });
	console.log(`Installed the Windows Desktop Client in ${output}.`);
}

/** Runs the client attached to this process and resolves with its exit code. */
function runClient(executable: string, env: NodeJS.ProcessEnv) {
	const client = spawn(executable, [], {
		cwd: windowsOutputDirectory(),
		env,
		stdio: "inherit",
	});
	return new Promise<number>((resolveExit) => {
		client.on("exit", (exitCode) => resolveExit(exitCode ?? 1));
		client.on("error", (error) => {
			console.error(error.message);
			resolveExit(1);
		});
	});
}

async function start(options: Set<string>) {
	const executable = installedExecutable();
	// `mise run start` stops its Music Server when this returns.
	if (options.has("--wait"))
		process.exit(await runClient(executable, process.env));
	// WSL hands its stdio pipes to a Windows process it starts, and a client
	// started through `cmd /c start` inherits them, so the interop call does
	// not return until the client exits; a detached one dies with a one-off
	// `wsl -e` session. PowerShell's Start-Process launches through the shell,
	// passing no handles, and returns at once.
	const quoted = (path: string) => `'${path.replaceAll("'", "''")}'`;
	const windowsPath = (path: string) =>
		execFileSync("wslpath", ["-w", path], { encoding: "utf8" }).trim();
	const result = spawnSync(
		"powershell.exe",
		[
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			`Start-Process -FilePath ${quoted(windowsPath(executable))} -WorkingDirectory ${quoted(windowsPath(windowsOutputDirectory()))}`,
		],
		{ cwd: windowsOutputDirectory(), stdio: "ignore", timeout: 30_000 },
	);
	if (result.status !== 0) {
		throw new Error(`Harmony could not be started (${result.status}).`);
	}
}

/** Stops pnpm and the Vite it started; they share a process group. */
function stopGroup(child: ChildProcess) {
	if (child.pid === undefined || child.exitCode !== null) return;
	try {
		process.kill(-child.pid, "SIGTERM");
	} catch {
		// Already gone.
	}
}

async function waitForDevServer(web: ChildProcess) {
	for (let attempt = 1; attempt <= devServerStartupAttempts; attempt += 1) {
		if (web.exitCode !== null) throw new Error("Vite dev server exited.");
		try {
			await fetch(devServerUrl);
			return;
		} catch {
			await new Promise((wait) => setTimeout(wait, 100));
		}
	}
	throw new Error(`Vite dev server did not answer at ${devServerUrl}.`);
}

async function dev() {
	const executable = installedExecutable();
	const output = windowsOutputDirectory();
	buildShell();
	const core = buildCore("debug");
	replaceInstalled(output, () => {
		const shell = join(output, "resources/app/dist");
		rmSync(shell, { recursive: true, force: true });
		cpSync(join(repoRoot, "desktop/dist"), shell, { recursive: true });
		copyFileSync(core, join(output, "resources/core", coreName));
	});

	const web = spawn("pnpm", ["--dir", "web", "run", "dev"], {
		cwd: repoRoot,
		detached: true,
		stdio: "inherit",
	});
	const stop = () => stopGroup(web);
	process.on("exit", stop);
	process.on("SIGINT", () => process.exit(130));
	process.on("SIGTERM", () => process.exit(143));
	await waitForDevServer(web);

	// WSLENV lists the variables interop passes to Windows processes. No
	// `/u` flag: WSL 2.6 drops a variable marked WSL-to-Windows-only. The
	// owner's Sentry settings reach the core only when set (ADR 0033).
	const sentry = [
		"SENTRY_DSN",
		"SENTRY_ENVIRONMENT",
		"SENTRY_TRACES_SAMPLE_RATE",
	].filter((name) => process.env[name]);
	const wslEnv = [process.env.WSLENV, "EARTHLY_AUDIO_DEV_SERVER_URL", ...sentry]
		.filter(Boolean)
		.join(":");
	process.exit(
		await runClient(executable, {
			...process.env,
			EARTHLY_AUDIO_DEV_SERVER_URL: devServerUrl,
			WSLENV: wslEnv,
		}),
	);
}

function clean() {
	rmSync(stagingDirectory, { recursive: true, force: true });
	removeWindowsDirectory(windowsOutputDirectory());
}

const commands: Record<
	string,
	{ run: (options: Set<string>) => void | Promise<void>; options: string[] }
> = {
	build: { run: build, options: ["--web-built", "--installer"] },
	start: { run: start, options: ["--wait"] },
	dev: { run: dev, options: [] },
	clean: { run: clean, options: [] },
};
const usage =
	"Usage: node scripts/desktop-windows/cli.ts build [--web-built] [--installer]|start [--wait]|dev|clean";
const [command = "", ...options] = process.argv.slice(2);
const selected = commands[command];
if (!selected) {
	console.error(command ? `Unknown command: ${command}\n${usage}` : usage);
	process.exit(2);
}
const unknown = options.filter((option) => !selected.options.includes(option));
if (unknown.length > 0) {
	console.error(
		`Unknown option for ${command}: ${unknown.join(" ")}\n${usage}`,
	);
	process.exit(2);
}
try {
	await selected.run(new Set(options));
} catch (error) {
	console.error(error instanceof Error ? error.message : error);
	process.exit(1);
}
