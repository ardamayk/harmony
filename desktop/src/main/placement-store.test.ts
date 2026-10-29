import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vite-plus/test";
import { PlacementStore } from "./placement-store";

describe("PlacementStore", () => {
	it("round-trips the mini player placement and reads corrupt files as none", () => {
		const directory = mkdtempSync(join(tmpdir(), "earthly-mini-"));
		const path = join(directory, "nested/mini-window.json");
		const store = new PlacementStore(path);
		try {
			expect(store.load()).toBeNull();

			store.save({ x: 120, y: -40 });
			expect(store.load()).toEqual({ x: 120, y: -40 });

			writeFileSync(path, "not json");
			expect(store.load()).toBeNull();

			writeFileSync(path, JSON.stringify({ x: 1.5, y: 2 }));
			expect(store.load()).toBeNull();

			writeFileSync(path, JSON.stringify({ x: "1", y: 2 }));
			expect(store.load()).toBeNull();

			writeFileSync(path, JSON.stringify([1, 2]));
			expect(store.load()).toBeNull();
		} finally {
			rmSync(directory, { recursive: true, force: true });
		}
	});
});
