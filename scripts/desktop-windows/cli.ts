// Builds the Windows Desktop Client from WSL and runs it on the Windows side.
//   build  Web Client, shell bundle, release core cross-built with mingw-w64,
//          pinned mpv; electron-builder's `dir` target, copied to the
//          Windows output folder (%LOCALAPPDATA%\EarthlyAudio by default)
//   start  the built client
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

const executableName = "Earthly Audio.exe";
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
		`${directory} is in use; quit Earthly Audio (tray, Quit) and try again.`,
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
			`Missing ${executable}; run 'mise run desktop:windows' first.`,
		);
	}
	return executable;
}

async function build() {
	buildWeb();
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
		"dir",
		"--x64",
		"--config",
		"electron-builder.yml",
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

function start() {
	const executable = installedExecutable();
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
		throw new Error(`Earthly Audio could not be started (${result.status}).`);
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
	// `/u` flag: WSL 2.6 drops a variable marked WSL-to-Windows-only.
	const wslEnv = [process.env.WSLENV, "EARTHLY_AUDIO_DEV_SERVER_URL"]
		.filter(Boolean)
		.join(":");
	const client = spawn(executable, [], {
		cwd: output,
		env: {
			...process.env,
			EARTHLY_AUDIO_DEV_SERVER_URL: devServerUrl,
			WSLENV: wslEnv,
		},
		stdio: "inherit",
	});
	const code = await new Promise<number>((resolveExit) => {
		client.on("exit", (exitCode) => resolveExit(exitCode ?? 1));
		client.on("error", (error) => {
			console.error(error.message);
			resolveExit(1);
		});
	});
	process.exit(code);
}

function clean() {
	rmSync(stagingDirectory, { recursive: true, force: true });
	removeWindowsDirectory(windowsOutputDirectory());
}

const commands: Record<string, () => void | Promise<void>> = {
	build,
	start,
	dev,
	clean,
};
const usage =
	"Usage: node scripts/desktop-windows/cli.ts build|start|dev|clean";
const command = process.argv[2] ?? "";
const selected = commands[command];
if (!selected) {
	console.error(command ? `Unknown command: ${command}\n${usage}` : usage);
	process.exit(2);
}
try {
	await selected();
} catch (error) {
	console.error(error instanceof Error ? error.message : error);
	process.exit(1);
}
