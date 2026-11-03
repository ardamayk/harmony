import assert from "node:assert/strict";
import { test } from "node:test";
import { DEMO_ALBUMS, lrc, synthesize } from "./seed-demo-library.ts";

test("every demo Track has 30–60 s of audio and LRC lyrics inside it", () => {
	for (const album of DEMO_ALBUMS) {
		album.tracks.forEach((track, index) => {
			const seconds = synthesize(album, index).length / 44_100;
			assert.ok(seconds >= 30 && seconds <= 60, `${track.title}: ${seconds} s`);
			const times = lrc(album, track)
				.split("\n")
				.map((line) => {
					const match = /^\[(\d\d):(\d\d\.\d\d)\] \S/.exec(line);
					assert.ok(match, `${track.title}: ${line}`);
					return Number(match[1]) * 60 + Number(match[2]);
				});
			assert.ok(times.length >= 4 && times.at(-1)! < seconds);
		});
	}
});
