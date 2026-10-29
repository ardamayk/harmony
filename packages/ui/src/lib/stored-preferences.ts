// Device-local preferences kept in localStorage. Stored values may be old,
// hand-edited or corrupt; each parser falls back to its default instead of
// throwing.

import * as z from "zod";

/** "false" unpins the Queue Drawer; anything else (or nothing) pins it. */
const queuePinned = z
	.enum(["true", "false"])
	.transform((value) => value === "true")
	.catch(true);

/** Most recent first: Playlist ids the Player Bar offers to add to. */
const recentPlaylistIds = z.array(z.string()).catch([]);

export function parseQueuePinned(raw: string | null): boolean {
	return queuePinned.parse(raw);
}

export function parseRecentPlaylistIds(raw: string | null): string[] {
	if (!raw) return [];
	let value: unknown;
	try {
		value = JSON.parse(raw);
	} catch {
		return [];
	}
	return recentPlaylistIds.parse(value);
}
