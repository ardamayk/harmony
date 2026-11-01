import { ApiError, type CoverSize, type PlaybackState } from "@repo/api-client";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	browserDeviceName,
	claimBrowserDeviceId,
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

const openChannels: BroadcastChannel[] = [];

/** Closing a page closes its channels; a page that is gone answers nobody. */
class PageChannel extends BroadcastChannel {
	constructor(name: string) {
		super(name);
		openChannels.push(this);
	}
}

function closePages() {
	for (const channel of openChannels.splice(0)) channel.close();
}

/** A tab's `sessionStorage`; a browser copies it when it duplicates a tab. */
function tabStorage(copied: Iterable<[string, string]> = []) {
	const values = new Map(copied);
	return {
		getItem: (key: string) => values.get(key) ?? null,
		setItem: (key: string, value: string) => void values.set(key, value),
		[Symbol.iterator]: () => values[Symbol.iterator](),
	};
}

beforeEach(() => {
	vi.stubGlobal("BroadcastChannel", PageChannel);
});

afterEach(() => {
	closePages();
	sessionStorage.clear();
	vi.unstubAllGlobals();
});

describe("browser shared playback transport", () => {
	it("keeps a tab's device ID across a reload", async () => {
		const tab = tabStorage();
		const id = await claimBrowserDeviceId(tab);
		closePages();

		await expect(claimBrowserDeviceId(tab)).resolves.toBe(id);
	});

	it("answers a reload at once, without waiting for other tabs", async () => {
		const tab = tabStorage();
		const id = await claimBrowserDeviceId(tab);

		vi.useFakeTimers();
		await expect(claimBrowserDeviceId(tab, true)).resolves.toBe(id);
		vi.useRealTimers();
	});

	it("gives a duplicated tab its own device ID, which a reload keeps", async () => {
		const original = tabStorage();
		const originalId = await claimBrowserDeviceId(original);
		const duplicate = tabStorage(original);

		const duplicateId = await claimBrowserDeviceId(duplicate);

		expect(duplicateId).not.toBe(originalId);
		closePages();
		await expect(claimBrowserDeviceId(original)).resolves.toBe(originalId);
		await expect(claimBrowserDeviceId(duplicate)).resolves.toBe(duplicateId);
	});

	it("keeps the original's ID when it reloads beside its duplicate", async () => {
		const original = tabStorage();
		const originalId = await claimBrowserDeviceId(original);
		const duplicateId = await claimBrowserDeviceId(tabStorage(original));
		const [firstChannel] = openChannels;
		firstChannel?.close();

		await expect(claimBrowserDeviceId(original)).resolves.toBe(originalId);
		expect(originalId).not.toBe(duplicateId);
	});

	it("moves exactly one of two tabs opening with the same ID", async () => {
		const stored = tabStorage();
		const id = await claimBrowserDeviceId(stored);
		closePages();

		for (let round = 0; round < 5; round += 1) {
			const ids = await Promise.all([
				claimBrowserDeviceId(tabStorage(stored)),
				claimBrowserDeviceId(tabStorage(stored)),
			]);
			closePages();

			expect(ids.filter((claimed) => claimed === id)).toHaveLength(1);
		}
	});

	it("claims once per page, however often the device is asked for", async () => {
		const transport = createBrowserSharedPlaybackTransport();

		const first = await transport.getDevice();
		const second = await transport.getDevice();
		const third = await createBrowserSharedPlaybackTransport().getDevice();

		expect(first.id).not.toBe("");
		expect(second.id).toBe(first.id);
		expect(third.id).toBe(first.id);
	});

	it("makes a device ID on plain HTTP, where randomUUID does not exist", async () => {
		vi.stubGlobal("crypto", {
			getRandomValues: (bytes: Uint8Array) => bytes.fill(171),
		});

		await expect(claimBrowserDeviceId(tabStorage())).resolves.toBe(
			"ab".repeat(16),
		);
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
