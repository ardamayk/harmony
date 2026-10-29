import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import * as z from "zod";

const windowPlacement = z.object({ x: z.int(), y: z.int() });

export type WindowPlacement = z.infer<typeof windowPlacement>;

/**
 * Remembers where the mini player was last placed, next to the core's
 * settings files (`mini-window.json`, the file the Tauri client used).
 */
export class PlacementStore {
	constructor(private readonly path: string) {}

	load(): WindowPlacement | null {
		try {
			const parsed = windowPlacement.safeParse(
				JSON.parse(readFileSync(this.path, "utf8")),
			);
			if (parsed.success) return parsed.data;
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
