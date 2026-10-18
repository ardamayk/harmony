import { describe, expect, it } from "vite-plus/test";
import { activeLyricIndex, parseLyrics } from "./lyrics";

describe("parseLyrics", () => {
	it("reads LRC timestamps as synced lines in time order", () => {
		const parsed = parseLyrics(
			"[ar:Skillet]\n[00:22.13] First line\n[00:30.00][01:10.50] Chorus\n[00:25.67] Second line",
		);
		expect(parsed).toEqual({
			synced: true,
			lines: [
				{ time: 22.13, text: "First line" },
				{ time: 25.67, text: "Second line" },
				{ time: 30, text: "Chorus" },
				{ time: 70.5, text: "Chorus" },
			],
		});
	});

	it("keeps plain lines and collapses blank runs", () => {
		expect(parseLyrics("One\n\n\n\nTwo\r\n")).toEqual({
			synced: false,
			lines: [{ text: "One" }, { text: "" }, { text: "Two" }, { text: "" }],
		});
	});

	it("returns null for empty lyrics", () => {
		expect(parseLyrics("")).toBeNull();
		expect(parseLyrics("\n  \n")).toBeNull();
	});
});

describe("activeLyricIndex", () => {
	const lines = [
		{ time: 10, text: "a" },
		{ time: 20, text: "b" },
	];

	it("finds the last line that has started", () => {
		expect(activeLyricIndex(lines, 5)).toBe(-1);
		expect(activeLyricIndex(lines, 10)).toBe(0);
		expect(activeLyricIndex(lines, 19.9)).toBe(0);
		expect(activeLyricIndex(lines, 300)).toBe(1);
	});
});
