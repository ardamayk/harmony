// Builds the Web Client and the Electron shell bundle the Windows package ships.

import { spawnSync } from "node:child_process";
import { rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function pnpm(args: string[]) {
	const result = spawnSync("pnpm", args, { cwd: repoRoot, stdio: "inherit" });
	if (result.status !== 0) {
		throw new Error(
			`pnpm ${args.join(" ")} failed (${result.error?.message ?? result.status}).`,
		);
	}
}

/** Builds web/dist, which the package carries as resources/web/dist. */
export function buildWeb() {
	pnpm(["--dir", "web", "run", "build"]);
}

/** Bundles desktop/dist; a stale bundle must not outlive a failed pack. */
export function buildShell() {
	rmSync(join(repoRoot, "desktop/dist"), { recursive: true, force: true });
	pnpm(["--dir", "desktop", "exec", "vp", "pack"]);
}
