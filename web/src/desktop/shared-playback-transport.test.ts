import { ApiError, type PlaybackState, type Track } from "@repo/api-client";
import { DESKTOP_EVENTS } from "@repo/desktop-bridge";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const { invokeMock, listenMock } = vi.hoisted(() => ({
	invokeMock: vi.fn(),
	listenMock: vi.fn(),
}));

vi.mock("./desktop-api", () => ({
	invoke: invokeMock,
	listen: listenMock,
}));

vi.mock("#/lib/api", () => ({
	apiClient: {},
}));

import { createDesktopSharedPlaybackTransport } from "./shared-playback-transport";

const track: Track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120_000,
	format: "flac",
	genres: [],
};

const snapshot: PlaybackState = {
	revision: 12,
	queueItemId: "item-1",
	trackId: "track-1",
	playing: true,
	positionMs: 42_000,
	positionAtMs: 1_000,
	activeDeviceId: "device-1",
	activeDeviceName: "Harmony on music-desk",
	remoteCommandRevision: 4,
	skipRequest: null,
	serverTimeMs: 1_500,
};

const notFound = {
	error: "not_found",
	code: "not_found",
	message: "Track not found",
};

function createApi() {
	return {
		getTrack: vi.fn(async (trackId: string) => {
			if (trackId === "gone") throw new ApiError(404, notFound);
			return { ...track, id: trackId };
		}),
		getTrackStreamUrl: vi.fn(
			(trackId: string) =>
				`http://127.0.0.1:43129/proxy/api/v1/tracks/${trackId}/stream`,
		),
		getAlbumCoverUrl: vi.fn(
			(albumId: string, size: number) =>
				`http://127.0.0.1:43129/proxy/api/v1/library/albums/${albumId}/cover?size=${size}`,
		),
	};
}

describe("desktop shared playback transport", () => {
	beforeEach(() => {
		invokeMock.mockReset();
		listenMock.mockReset();
	});

	it("reads the device identity from the core", async () => {
		invokeMock.mockResolvedValue({
			id: "device-1",
			name: "Harmony on music-desk",
		});
		const transport = createDesktopSharedPlaybackTransport(createApi());

		await expect(transport.getDevice()).resolves.toEqual({
			id: "device-1",
			name: "Harmony on music-desk",
		});
		expect(invokeMock).toHaveBeenCalledWith("get_shared_playback_device");
	});

	it("sends commands through the core and resolves with the state", async () => {
		invokeMock.mockResolvedValue(snapshot);
		const transport = createDesktopSharedPlaybackTransport(createApi());
		const command = {
			type: "claim",
			deviceId: "device-1",
			deviceName: "Harmony on music-desk",
			queueItemId: null,
			trackId: null,
			positionMs: 0,
			playing: true,
		} as const;

		await expect(transport.sendCommand(command)).resolves.toEqual(snapshot);
		expect(invokeMock).toHaveBeenCalledWith("send_shared_playback_command", {
			command,
		});
	});

	it("subscribes to the core's shared playback snapshots and unsubscribes", () => {
		const unlisten = vi.fn();
		listenMock.mockReturnValue(unlisten);
		const transport = createDesktopSharedPlaybackTransport(createApi());
		const onState = vi.fn();

		const stop = transport.subscribe(onState);
		expect(listenMock).toHaveBeenCalledWith(
			DESKTOP_EVENTS.sharedPlaybackState,
			onState,
		);
		stop();
		expect(unlisten).toHaveBeenCalledOnce();
	});

	it("resolves the latest snapshot and refuses to invent one", async () => {
		invokeMock.mockResolvedValue(snapshot);
		const transport = createDesktopSharedPlaybackTransport(createApi());
		await expect(transport.getState()).resolves.toEqual(snapshot);

		invokeMock.mockResolvedValue(null);
		await expect(transport.getState()).rejects.toThrow(
			"Desktop core has no Shared Playback State",
		);
	});

	it("builds the same Track source the Queue plays, with its queue item", async () => {
		const api = createApi();
		const transport = createDesktopSharedPlaybackTransport(api);

		const source = await transport.resolveSource(snapshot);
		expect(api.getTrack).toHaveBeenCalledWith("track-1");
		expect(source).toEqual({
			type: "track",
			track: { ...track, id: "track-1" },
			playbackUrl: "http://127.0.0.1:43129/proxy/api/v1/tracks/track-1/stream",
			queueItemId: "item-1",
			artworkUrl:
				"http://127.0.0.1:43129/proxy/api/v1/library/albums/album-1/cover?size=512",
		});
	});

	it("resolves null for a gone Track and for sources without one", async () => {
		const api = createApi();
		const transport = createDesktopSharedPlaybackTransport(api);

		await expect(
			transport.resolveSource({ ...snapshot, trackId: "gone" }),
		).resolves.toBeNull();
		await expect(
			transport.resolveSource({ ...snapshot, trackId: null }),
		).resolves.toBeNull();
		// A failed request says nothing about the Track.
		api.getTrack.mockRejectedValueOnce(new TypeError("offline"));
		await expect(transport.resolveSource(snapshot)).rejects.toThrow("offline");
		// A queue item that has since been removed keeps the source item-less.
		const source = await transport.resolveSource({
			...snapshot,
			queueItemId: null,
		});
		expect(source).toMatchObject({ queueItemId: undefined });
	});

	it("creates and subscribes without claiming", async () => {
		const api = createApi();
		const transport = createDesktopSharedPlaybackTransport(api);
		transport.subscribe(() => {});

		// The renderer does exactly this at startup;
		// it must not claim the Shared Playback State by itself.
		expect(invokeMock).not.toHaveBeenCalled();
		expect(api.getTrack).not.toHaveBeenCalled();
	});
});
