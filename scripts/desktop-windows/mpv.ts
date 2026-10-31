// Stages the pinned official mpv build for Windows beside the core. mpv.exe
// loads the DLLs next to it, so they always travel together.
import { spawnSync } from "node:child_process";
import {
	chmodSync,
	copyFileSync,
	mkdirSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { download, extractZip } from "./fetch.ts";

export type MpvRelease = { url: string; sha256: string; versionFile: string };

export async function stageMpv(
	destination: string,
	release: MpvRelease,
	cacheDirectory: string,
): Promise<void> {
	const pinned = readFileSync(release.versionFile, "utf8").trim();
	const archive = await download(release.url, release.sha256, cacheDirectory);
	const work = mkdtempSync(join(tmpdir(), "earthly-audio-mpv-"));
	try {
		const outer = join(work, "outer");
		extractZip(archive, outer);
		// The release zip wraps the build in one inner mpv-git-*-x86_64.zip.
		const inner = readdirSync(outer).filter((name) =>
			/^mpv-git-.*-x86_64\.zip$/.test(name),
		);
		if (inner.length !== 1) {
			throw new Error(
				`${archive}: expected one mpv-git-*-x86_64.zip, found ${inner.length}`,
			);
		}
		const build = join(work, "build");
		extractZip(join(outer, inner[0]), build);

		// Only the player and its DLLs: mpv.com and the .bat installers are
		// for a console install, not for a core that spawns mpv.exe.
		const files = readdirSync(build).filter(
			(name) => name === "mpv.exe" || name.toLowerCase().endsWith(".dll"),
		);
		if (!files.includes("mpv.exe")) throw new Error(`${archive}: no mpv.exe`);
		mkdirSync(destination, { recursive: true });
		for (const name of files) {
			copyFileSync(join(build, name), join(destination, name));
		}
		// zipfile drops the execute bit, and interop needs it to run the exe.
		chmodSync(join(destination, "mpv.exe"), 0o755);
	} finally {
		rmSync(work, { recursive: true, force: true });
	}

	// A Linux CI runner cannot start a Windows executable: a release relies on
	// the archive's SHA-256, and mpv-update.yml runs a new mpv on Windows first.
	if (process.env.CI && process.platform !== "win32") return;

	// Run the staged copy through WSL interop: this also proves the DLL set
	// is complete, since a missing DLL fails before --version prints.
	const mpv = join(destination, "mpv.exe");
	const result = spawnSync(mpv, ["--version"], { encoding: "utf8" });
	if (result.error) throw result.error;
	const firstLine = result.stdout.split(/\r?\n/, 1)[0] ?? "";
	if (result.status !== 0 || !firstLine.startsWith(`mpv v${pinned}`)) {
		throw new Error(
			`${mpv} --version: "${firstLine}" (exit ${result.status}), expected mpv v${pinned}`,
		);
	}
}
