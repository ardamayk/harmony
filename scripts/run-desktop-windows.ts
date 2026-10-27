// Windows counterpart of run-desktop.sh: builds and launches the Electron
// Desktop Client (desktop/) without bash.
//   dev    debug Rust core, Vite dev server with HMR, and the shell
//   build  Web Client, release Rust core (with the pinned mpv.exe beside it)
//          and the shell bundle
//   start  the built release client, rendering web/dist
//
// The Music Server is not started: point the connection screen at a running
// one (for example the WSL or Linux server on http://127.0.0.1:8090).
// Pinned mpv comes from EARTHLY_AUDIO_MPV_PATH, else desktop/core/binaries.
// mpv.exe loads the DLLs beside it, so it always moves with them.
// The core builds for x86_64-pc-windows-gnullvm with MSYS2's CLANG64 clang,
// which compiles its C dependencies (AWS-LC under rustls); MSYS2_ROOT
// overrides C:\msys64. EARTHLY_AUDIO_CORE_PATH runs a prebuilt core instead.
// Either way pinned mpv is placed beside the core, where it looks first.

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	rmSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktopDirectory = resolve(
	dirname(fileURLToPath(import.meta.url)),
	"../desktop",
);
const targetDirectory = resolve(
	process.env.CARGO_TARGET_DIR ?? join(desktopDirectory, "../target"),
);
const coreManifest = join(desktopDirectory, "core/Cargo.toml");
const stagedMpv = join(desktopDirectory, "core/binaries/mpv.exe");
const devServerUrl = "http://localhost:3000";
const devServerStartupAttempts = 300;
const coreTarget = "x86_64-pc-windows-gnullvm";
const clang64Directory = join(
	process.env.MSYS2_ROOT ?? "C:\\msys64",
	"clang64",
	"bin",
);

function run(
	command: string,
	args: string[],
	env: NodeJS.ProcessEnv = process.env,
) {
	// pnpm is a .cmd shim on Windows, which only a shell can start.
	const result = spawnSync(command, args, {
		cwd: desktopDirectory,
		env,
		stdio: "inherit",
		shell: command === "pnpm",
	});
	if (result.status !== 0) {
		throw new Error(`${command} ${args.join(" ")} failed (${result.status}).`);
	}
}

function pinnedMpvVersion(): string {
	// The official mpv 0.41.0 Windows release build reports its git version.
	return readFileSync(
		join(desktopDirectory, "core/mpv-windows-version.txt"),
		"utf8",
	)
		.replaceAll(/[\r\n]/g, "")
		.trim();
}

/** Validates pinned mpv and stages it where the debug core looks for it. */
function prepareMpvSidecar() {
	const source = process.env.EARTHLY_AUDIO_MPV_PATH ?? stagedMpv;
	if (!existsSync(source)) {
		throw new Error(
			`Pinned mpv.exe is missing at ${source}; set EARTHLY_AUDIO_MPV_PATH to it.`,
		);
	}
	const pinned = pinnedMpvVersion();
	const version = spawnSync(source, ["--version"], { encoding: "utf8" });
	const firstLine = (version.stdout ?? "").split(/\r?\n/, 1)[0] ?? "";
	if (
		!firstLine.startsWith(`mpv v${pinned} `) &&
		firstLine !== `mpv v${pinned}`
	) {
		throw new Error(
			`Desktop Client requires pinned mpv ${pinned}; found: ${firstLine || "no version"}`,
		);
	}
	copyMpvBundle(source, dirname(stagedMpv));
	console.log(`Staged pinned mpv ${pinned} for Windows.`);
}

/** Copies mpv.exe and the DLLs it loads from its own directory. */
function copyMpvBundle(mpv: string, directory: string) {
	const sourceDirectory = dirname(resolve(mpv));
	if (sourceDirectory === resolve(directory)) return;
	mkdirSync(directory, { recursive: true });
	copyFileSync(mpv, join(directory, "mpv.exe"));
	for (const name of readdirSync(sourceDirectory)) {
		if (name.toLowerCase().endsWith(".dll")) {
			copyFileSync(join(sourceDirectory, name), join(directory, name));
		}
	}
}

function prebuiltCore(): string | undefined {
	const core = process.env.EARTHLY_AUDIO_CORE_PATH;
	return core ? resolve(core) : undefined;
}

function buildCore(profile: "debug" | "release"): string {
	if (!existsSync(join(clang64Directory, "clang.exe"))) {
		throw new Error(
			`MSYS2 CLANG64 clang is missing in ${clang64Directory}; install MSYS2 and mingw-w64-clang-x86_64-clang, or set EARTHLY_AUDIO_CORE_PATH.`,
		);
	}
	const args = [
		"build",
		"--manifest-path",
		coreManifest,
		"--target",
		coreTarget,
	];
	if (profile === "release") args.push("--release");
	// Windows keeps PATH as "Path"; replace that key rather than adding a second.
	const pathKey =
		Object.keys(process.env).find((key) => key.toUpperCase() === "PATH") ??
		"PATH";
	run("cargo", args, {
		...process.env,
		[pathKey]: `${clang64Directory};${process.env[pathKey] ?? ""}`,
		CC_x86_64_pc_windows_gnullvm: "clang",
		AR_x86_64_pc_windows_gnullvm: "llvm-ar",
	});
	return join(targetDirectory, coreTarget, profile, "earthly-audio-core.exe");
}

function buildShell() {
	rmSync(join(desktopDirectory, "dist"), { recursive: true, force: true });
	run("pnpm", ["exec", "vp", "pack"]);
}

function electronExecutable(): string {
	// The electron package's main export is the path of its binary.
	return createRequire(join(desktopDirectory, "package.json"))(
		"electron",
	) as string;
}

function launchElectron(env: NodeJS.ProcessEnv): Promise<number> {
	const child = spawn(electronExecutable(), ["."], {
		cwd: desktopDirectory,
		env: { ...process.env, ...env },
		stdio: "inherit",
	});
	return new Promise((resolveExit) => {
		child.on("exit", (code) => resolveExit(code ?? 1));
	});
}

/** Stops a process and everything it started (pnpm → Vite). */
function stopTree(child: ChildProcess) {
	if (child.pid === undefined || child.exitCode !== null) return;
	spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
		stdio: "ignore",
	});
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
	prepareMpvSidecar();
	const core = prebuiltCore() ?? buildCore("debug");
	copyMpvBundle(stagedMpv, dirname(core));
	buildShell();
	const web = spawn("pnpm", ["--dir", "../web", "run", "dev"], {
		cwd: desktopDirectory,
		stdio: "inherit",
		shell: true,
	});
	const stop = () => stopTree(web);
	process.on("exit", stop);
	process.on("SIGINT", () => process.exit(130));
	await waitForDevServer(web);
	const code = await launchElectron({
		EARTHLY_AUDIO_CORE_PATH: core,
		EARTHLY_AUDIO_DEV_SERVER_URL: devServerUrl,
	});
	process.exit(code);
}

function build() {
	run("pnpm", ["--dir", "../web", "run", "build"]);
	prepareMpvSidecar();
	const core = prebuiltCore() ?? buildCore("release");
	copyMpvBundle(stagedMpv, dirname(core));
	buildShell();
}

async function start() {
	const core =
		prebuiltCore() ??
		join(targetDirectory, coreTarget, "release/earthly-audio-core.exe");
	for (const required of [
		core,
		join(desktopDirectory, "dist/main.cjs"),
		join(desktopDirectory, "../web/dist/index.html"),
	]) {
		if (!existsSync(required)) {
			throw new Error(
				`Missing ${required}; run 'pnpm --dir desktop run win:build' first.`,
			);
		}
	}
	process.exit(await launchElectron({ EARTHLY_AUDIO_CORE_PATH: core }));
}

const commands: Record<string, () => void | Promise<void>> = {
	dev,
	build,
	start,
};
const command = process.argv[2] ?? "";
const selected = commands[command];
if (process.platform !== "win32") {
	console.error("run-desktop-windows.ts is for Windows; use run-desktop.sh.");
	process.exit(2);
}
if (!selected) {
	console.error(`Unknown Desktop command: ${command || "(none)"}`);
	process.exit(2);
}
try {
	await selected();
} catch (error) {
	console.error(error instanceof Error ? error.message : error);
	process.exit(1);
}
