import { describe, expect, it } from "vite-plus/test";
import { parseQueuePinned, parseRecentPlaylistIds } from "./stored-preferences";

describe("parseQueuePinned", () => {
	it("reads the stored choice", () => {
		expect(parseQueuePinned("true")).toBe(true);
		expect(parseQueuePinned("false")).toBe(false);
	});

	it("pins the Queue Drawer when nothing usable is stored", () => {
		for (const raw of [null, "", "0", "FALSE", "no", '"false"', "{}"]) {
			expect(parseQueuePinned(raw)).toBe(true);
		}
	});
});

describe("parseRecentPlaylistIds", () => {
	it("reads the stored ids in order", () => {
		expect(parseRecentPlaylistIds('["p2","p1"]')).toEqual(["p2", "p1"]);
		expect(parseRecentPlaylistIds("[]")).toEqual([]);
	});

	it("starts over from no recent Playlists on missing, old or corrupt data", () => {
		for (const raw of [
			null,
			"",
			"not json",
			'"p1"',
			"null",
			'{"ids":["p1"]}',
			'["p1",2]',
			"[null]",
		]) {
			expect(parseRecentPlaylistIds(raw)).toEqual([]);
		}
	});
});
