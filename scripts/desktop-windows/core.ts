// Cross-builds the Rust core for Windows with WSL's mingw-w64 toolchain.

import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const target = "x86_64-pc-windows-gnu";

function run(command: string, args: string[]) {
	const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit" });
	if (result.status !== 0) {
		throw new Error(
			`${command} ${args.join(" ")} failed (${result.error?.message ?? result.status}).`,
		);
	}
}

/** Builds earthly-audio-core.exe and returns its path. */
export function buildCore(profile: "debug" | "release"): string {
	// A no-op when the target is installed; a fresh toolchain lacks its std.
	run("rustup", ["target", "add", target]);
	const args = ["build", "-p", "earthly-audio-core", "--target", target];
	if (profile === "release") args.push("--release");
	run("cargo", args);
	return join(repoRoot, "target", target, profile, "earthly-audio-core.exe");
}
