import assert from "node:assert/strict";
import { test } from "node:test";
import {
	buildDate,
	commitAtBuild,
	ffmpegCommit,
	reportedVersion,
	scriptVersion,
	Unproved,
	unknownLibraries,
} from "./mpv-update.ts";

test("reads the build's own identity out of its binaries", () => {
	assert.equal(
		ffmpegCommit("\0FFmpeg version git-2025-12-21-1fcb201\0"),
		"1fcb201",
	);
	assert.equal(
		reportedVersion("\0mpv v0.41.0-dev-g41f6a6450\0mpv protocol"),
		"0.41.0-dev-g41f6a6450",
	);
	assert.throws(() => ffmpegCommit("no version here"), Unproved);
});

test("reads pinned versions from the build script", () => {
	const script = "_fribidi () {\n    local ver=1.0.16\n";
	assert.equal(scriptVersion(script, "fribidi"), "1.0.16");
	assert.throws(() => scriptVersion(script, "iconv"), Unproved);
});

test("reads the build date from the inner archive name", () => {
	assert.equal(
		buildDate("mpv-git-2025-12-21-41f6a64-x86_64.zip"),
		"2025-12-21",
	);
});

test("a library without a known licence stops the update", () => {
	assert.deepEqual(
		unknownLibraries(["avcodec-62.dll", "libplacebo-358.dll", "mpv.exe"]),
		[],
	);
	assert.deepEqual(unknownLibraries(["zlib1.dll", "libnew-3.dll"]), ["libnew"]);
});

test("a libplacebo commit on the build date cannot be pinned", () => {
	assert.equal(commitAtBuild("47587a2d", "47587a2d"), "47587a2d");
	assert.throws(() => commitAtBuild("47587a2d", "bc90ef94"), Unproved);
});
