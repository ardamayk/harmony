// Picks the Desktop Client the desktop:* tasks build and run: the Windows
// one from WSL (scripts/desktop-windows/cli.ts), the Linux one everywhere
// else, CI included (scripts/run-desktop.sh). EARTHLY_DESKTOP_TARGET=linux
// or windows overrides the choice.
//
//   node scripts/desktop-target.ts   prints linux or windows

import { readFileSync } from "node:fs";

export type DesktopTarget = "linux" | "windows";

type Environment = Record<string, string | undefined>;

/** WSL sets WSL_DISTRO_NAME; its kernel names Microsoft in /proc/version. */
export function isWsl(env: Environment, procVersion: string | undefined) {
	return Boolean(env.WSL_DISTRO_NAME) || /microsoft/i.test(procVersion ?? "");
}

export function selectDesktopTarget(
	env: Environment,
	procVersion: string | undefined,
): DesktopTarget {
	const wsl = isWsl(env, procVersion);
	const override = env.EARTHLY_DESKTOP_TARGET;
	if (!override) return wsl ? "windows" : "linux";
	if (override === "linux") return "linux";
	if (override === "windows") {
		if (!wsl) {
			throw new Error(
				"EARTHLY_DESKTOP_TARGET=windows works only in WSL: the Windows Desktop Client is built and started from WSL.",
			);
		}
		return "windows";
	}
	throw new Error(
		`EARTHLY_DESKTOP_TARGET must be linux or windows, not "${override}".`,
	);
}

function readProcVersion(): string | undefined {
	try {
		return readFileSync("/proc/version", "utf8");
	} catch {
		return undefined;
	}
}

if (import.meta.main) {
	try {
		console.log(selectDesktopTarget(process.env, readProcVersion()));
	} catch (error) {
		console.error(error instanceof Error ? error.message : error);
		process.exit(2);
	}
}
