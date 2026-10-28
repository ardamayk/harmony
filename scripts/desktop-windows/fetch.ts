// Pinned downloads for the Windows Desktop Client build: each file is cached
// by name and trusted only while its SHA-256 matches the pin.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	readFileSync,
	renameSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { basename, join } from "node:path";

export function sha256File(path: string): string {
	return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/** Returns the cached file's path, downloading it when missing or altered. */
export async function download(
	url: string,
	sha256: string,
	cacheDirectory: string,
): Promise<string> {
	const expected = sha256.toLowerCase();
	const target = join(cacheDirectory, basename(new URL(url).pathname));
	// Re-hash every time: a truncated or tampered cache entry must not pass.
	if (existsSync(target) && sha256File(target) === expected) return target;

	mkdirSync(cacheDirectory, { recursive: true });
	// fetch follows redirects (GitHub release assets redirect to a CDN).
	const response = await fetch(url);
	if (!response.ok) {
		throw new Error(`download ${url}: HTTP ${response.status}`);
	}
	// Pinned archives are tens of MB: buffering is simpler than streaming.
	const data = Buffer.from(await response.arrayBuffer());
	const actual = createHash("sha256").update(data).digest("hex");
	if (actual !== expected) {
		throw new Error(`download ${url}: sha256 ${actual}, expected ${expected}`);
	}
	// A temp name keeps an interrupted write from looking cached.
	const partial = `${target}.${process.pid}.partial`;
	try {
		writeFileSync(partial, data);
		renameSync(partial, target);
	} finally {
		rmSync(partial, { force: true });
	}
	return target;
}

/** Extracts with Python's zipfile: unzip is not always installed in WSL. */
export function extractZip(archive: string, destination: string): void {
	mkdirSync(destination, { recursive: true });
	const result = spawnSync(
		"python3",
		["-I", "-m", "zipfile", "-e", archive, destination],
		{ stdio: "inherit" },
	);
	if (result.error) throw result.error;
	if (result.status !== 0) {
		throw new Error(`extract ${archive}: python3 exited ${result.status}`);
	}
}
