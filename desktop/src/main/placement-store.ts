import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export type WindowPlacement = { x: number; y: number };

/**
 * Remembers where the mini player was last placed, next to the core's
 * settings files (`mini-window.json`, the file the Tauri client used).
 */
export class PlacementStore {
	constructor(private readonly path: string) {}

	load(): WindowPlacement | null {
		try {
			const value: unknown = JSON.parse(readFileSync(this.path, "utf8"));
			if (isPlacement(value)) return { x: value.x, y: value.y };
		} catch {
			// Missing or corrupt: no remembered placement.
		}
		return null;
	}

	save(placement: WindowPlacement): void {
		mkdirSync(dirname(this.path), { recursive: true });
		writeFileSync(this.path, JSON.stringify(placement));
	}
}

function isPlacement(value: unknown): value is WindowPlacement {
	if (typeof value !== "object" || value === null) return false;
	const { x, y } = value as Record<string, unknown>;
	return Number.isInteger(x) && Number.isInteger(y);
}
