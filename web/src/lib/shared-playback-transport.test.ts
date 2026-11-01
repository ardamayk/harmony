import { ApiError, type CoverSize, type PlaybackState } from "@repo/api-client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	browserDeviceName,
	createBrowserSharedPlaybackTransport,
	resolveSharedPlaybackSource,
} from "./shared-playback-transport";

const snapshot = {
	queueItemId: "item-1",
	trackId: "track-1",
} as PlaybackState;

const api = {
	getTrack: vi.fn(async () => ({ id: "track-1", albumId: "album-1" }) as never),
	getTrackStreamUrl: (trackId: string) => `/stream/${trackId}`,
	getAlbumCoverUrl: (albumId: string, size: CoverSize) =>
		`/cover/${albumId}/${size}`,
};

afterEach(() => {
	sessionStorage.clear();
	vi.unstubAllGlobals();
});

describe("browser shared playback transport", () => {
	it("keeps one device ID per tab across reloads", async () => {
		const first = await createBrowserSharedPlaybackTransport().getDevice();
		const second = await createBrowserSharedPlaybackTransport().getDevice();

		expect(first.id).not.toBe("");
		expect(second.id).toBe(first.id);
	});

	it("makes a device ID on plain HTTP, where randomUUID does not exist", async () => {
		vi.stubGlobal("crypto", {
			getRandomValues: (bytes: Uint8Array) => bytes.fill(171),
		});

		const { id } = await createBrowserSharedPlaybackTransport().getDevice();

		expect(id).toBe("ab".repeat(16));
	});

	it.each([
		[
			"Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0",
			"Firefox on Linux",
		],
		[
			"Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 Chrome/128.0 Safari/537.36 Edg/128.0",
			"Edge on Windows",
		],
		[
			"Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/128.0 Mobile Safari/537.36",
			"Chrome on Android",
		],
		[
			"Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Safari/604.1",
			"Safari on iOS",
		],
		["Unknown", "Browser"],
	])("names %s", (userAgent, name) => {
		expect(browserDeviceName(userAgent)).toBe(name);
	});

	it("builds the source a Queue item plays", async () => {
		await expect(resolveSharedPlaybackSource(snapshot, api)).resolves.toEqual({
			type: "track",
			track: { id: "track-1", albumId: "album-1" },
			playbackUrl: "/stream/track-1",
			queueItemId: "item-1",
			artworkUrl: "/cover/album-1/512",
		});
	});

	it("resolves null without a Track and for a Track that is gone", async () => {
		await expect(
			resolveSharedPlaybackSource({ ...snapshot, trackId: null }, api),
		).resolves.toBeNull();
		api.getTrack.mockRejectedValueOnce(
			new ApiError(404, { error: "not_found", code: "not_found", message: "" }),
		);
		await expect(
			resolveSharedPlaybackSource(snapshot, api),
		).resolves.toBeNull();
	});

	it("rethrows a failed request, which says nothing about the Track", async () => {
		api.getTrack.mockRejectedValueOnce(new TypeError("offline"));
		await expect(resolveSharedPlaybackSource(snapshot, api)).rejects.toThrow(
			"offline",
		);
	});
});
