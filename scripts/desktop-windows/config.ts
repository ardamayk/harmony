// Where the Windows Desktop Client build reads its inputs and puts its output.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = resolve(
	dirname(fileURLToPath(import.meta.url)),
	"../..",
);
export const cacheDirectory = join(homedir(), ".cache/earthly-audio-windows");
export const coreTarget = "x86_64-pc-windows-gnu";
export const stagingDirectory = join(repoRoot, "target/desktop-windows");
export const stagedCoreDirectory = join(stagingDirectory, "core");

/** The official mpv Windows release build the package ships beside the core. */
export const mpvRelease = {
	url: "https://github.com/mpv-player/mpv/releases/download/v0.41.0/mpv-v0.41.0-x86_64-w64-mingw32.zip",
	sha256: "a49811c0752c108b8260636f9c6f6fcb97406641c98b30f1e7b500dfb20177de",
	versionFile: join(repoRoot, "desktop/core/mpv-windows-version.txt"),
};

export function electronVersion(): string {
	const manifest = JSON.parse(
		readFileSync(join(repoRoot, "desktop/package.json"), "utf8"),
	) as { devDependencies: Record<string, string> };
	return manifest.devDependencies.electron;
}

function wslPath(windowsPath: string): string {
	return execFileSync("wslpath", ["-u", windowsPath], {
		encoding: "utf8",
	}).trim();
}

let outputDirectory: string | undefined;

/** The Windows folder the client runs from, as a WSL path. */
export function windowsOutputDirectory(): string {
	if (outputDirectory !== undefined) return outputDirectory;
	const override = process.env.EARTHLY_AUDIO_WINDOWS_OUT;
	if (override) {
		// Accept the Windows form too, since that is what Explorer shows.
		outputDirectory = /^[A-Za-z]:[\\/]/.test(override)
			? wslPath(override)
			: resolve(override);
		return outputDirectory;
	}
	// cmd.exe warns about UNC paths when started in a Linux directory.
	const localAppData = execFileSync("cmd.exe", ["/c", "echo %LOCALAPPDATA%"], {
		cwd: "/mnt/c",
		encoding: "utf8",
	}).trim();
	if (!/^[A-Za-z]:\\/.test(localAppData)) {
		throw new Error(
			`Could not read %LOCALAPPDATA% through WSL interop (got "${localAppData}"); set EARTHLY_AUDIO_WINDOWS_OUT.`,
		);
	}
	outputDirectory = join(wslPath(localAppData), "EarthlyAudio");
	return outputDirectory;
}
