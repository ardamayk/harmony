import type { QueueItem } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import { findCurrentQueueIndex } from "./queue-groups";

const track = {
	id: "track-1",
	title: "Track",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "opus",
	genres: [],
};
const queue: QueueItem[] = [
	{ id: "new-item-1", trackId: track.id, position: 0, track },
	{ id: "new-item-2", trackId: track.id, position: 1, track },
];

describe("findCurrentQueueIndex", () => {
	it("falls back to the playing track after queue item IDs change", () => {
		expect(findCurrentQueueIndex(queue, "stale-item", track.id)).toBe(0);
	});
	it("prefers the exact queue item for duplicate tracks", () => {
		expect(findCurrentQueueIndex(queue, "new-item-2", track.id)).toBe(1);
	});
	it("locates a track without a queue item ID", () => {
		expect(findCurrentQueueIndex(queue, undefined, track.id)).toBe(0);
	});
	it("returns no position when neither identity matches", () => {
		expect(findCurrentQueueIndex(queue, "stale-item", "missing")).toBe(-1);
		expect(findCurrentQueueIndex(queue, "stale-item")).toBe(-1);
		expect(findCurrentQueueIndex([], "stale-item", track.id)).toBe(-1);
	});
});
