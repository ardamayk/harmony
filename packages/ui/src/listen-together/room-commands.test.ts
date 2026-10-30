import type { ListenTogetherRoom } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import { playTrackCommand } from "./room-commands";

const room: ListenTogetherRoom = {
	revision: 1,
	trackIds: ["a", "b", "c"],
	currentIndex: 1,
	playing: true,
	positionMs: 0,
	positionAtMs: 0,
	lyricsOpen: false,
	guestCount: 1,
	maxGuests: 8,
	expiresAt: "2026-10-31T12:00:00Z",
};

describe("playTrackCommand", () => {
	it("makes a context the Room Queue", () => {
		expect(playTrackCommand(room, "y", ["x", "y", "z"])).toEqual({
			type: "playTracks",
			trackIds: ["x", "y", "z"],
			index: 1,
		});
	});

	it("puts a Track missing from its context first", () => {
		expect(playTrackCommand(room, "y", ["x"])).toEqual({
			type: "playTracks",
			trackIds: ["y", "x"],
			index: 0,
		});
	});

	it("plays a queued Track from its place", () => {
		expect(playTrackCommand(room, "c")).toEqual({
			type: "playTracks",
			trackIds: ["a", "b", "c"],
			index: 2,
		});
	});

	it("plays a lone Track after the current one, keeping the Room Queue", () => {
		expect(playTrackCommand(room, "x")).toEqual({
			type: "playTracks",
			trackIds: ["a", "b", "x", "c"],
			index: 2,
		});
	});

	it("starts an empty Room", () => {
		expect(
			playTrackCommand({ ...room, trackIds: [], currentIndex: null }, "x"),
		).toEqual({ type: "playTracks", trackIds: ["x"], index: 0 });
	});

	it("cuts a context to what a Room Queue holds, from the Track on", () => {
		const ids = Array.from({ length: 700 }, (_, index) => `t${index}`);
		const command = playTrackCommand(room, "t100", ids);
		expect(command).toMatchObject({ type: "playTracks", index: 0 });
		const trackIds = command.type === "playTracks" ? command.trackIds : [];
		expect(trackIds).toHaveLength(500);
		expect(trackIds[0]).toBe("t100");
	});
});
