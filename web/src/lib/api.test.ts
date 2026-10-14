import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const { desktopFetchMock, listenForQueueEventsMock, unlistenQueueEventsMock } =
	vi.hoisted(() => ({
		desktopFetchMock: vi.fn(),
		listenForQueueEventsMock: vi.fn(),
		unlistenQueueEventsMock: vi.fn(),
	}));

vi.mock("#/desktop/bridge", () => ({
	desktopFetch: desktopFetchMock,
	getCoverBaseUrl: () => "http://earthly-media.localhost",
	getMediaProxyBaseUrl: () => "http://127.0.0.1:41000/token-1",
	isDesktopClient: () => true,
	listenForQueueEvents: listenForQueueEventsMock,
}));

import { apiClient } from "./api";

describe("desktop API client", () => {
	beforeEach(() => {
		desktopFetchMock.mockReset();
		listenForQueueEventsMock.mockReset();
		unlistenQueueEventsMock.mockReset();
	});

	it("uses native Queue event subscription", async () => {
		desktopFetchMock.mockResolvedValue(
			serverHealthResponse(["api.v1", "playback.queue-events.v1"]),
		);
		listenForQueueEventsMock.mockResolvedValue(unlistenQueueEventsMock);

		const unsubscribe = apiClient.subscribePlaybackQueueEvents(vi.fn());
		await vi.waitFor(() => expect(listenForQueueEventsMock).toHaveBeenCalled());
		unsubscribe();

		expect(unlistenQueueEventsMock).toHaveBeenCalledOnce();
	});

	it("does not start native Queue events when capability is absent", async () => {
		desktopFetchMock.mockResolvedValue(serverHealthResponse(["api.v1"]));
		const onError = vi.fn();

		apiClient.subscribePlaybackQueueEvents(vi.fn(), onError);

		await vi.waitFor(() => expect(onError).toHaveBeenCalled());
		expect(onError.mock.calls[0]?.[0].message).toContain(
			"playback.queue-events.v1",
		);
		expect(listenForQueueEventsMock).not.toHaveBeenCalled();
	});

	it("routes covers through the bounded protocol and streams through the proxy", () => {
		expect(apiClient.getAlbumCoverUrl("album-1")).toBe(
			"http://earthly-media.localhost/api/v1/library/albums/album-1/cover",
		);
		expect(apiClient.getTrackStreamUrl("track-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/tracks/track-1/stream",
		);
		expect(apiClient.getRadioStationStreamUrl("station-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/stations/station-1/stream",
		);
		expect(apiClient.getRadioCatalogPreviewStreamUrl("preview-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/preview/preview-1/stream",
		);
	});
});

function serverHealthResponse(capabilities: string[]) {
	return new Response(
		JSON.stringify({ status: "ok", version: "0.1.0", capabilities }),
		{
			status: 200,
			headers: { "Content-Type": "application/json" },
		},
	);
}
