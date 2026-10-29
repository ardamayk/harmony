import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const { invokeMock, listenMock } = vi.hoisted(() => ({
	invokeMock: vi.fn(),
	listenMock: vi.fn(),
}));

vi.mock("./desktop-api", () => ({
	hasDesktopBridge: () => true,
	invoke: invokeMock,
	listen: listenMock,
}));

import {
	cancelDesktopImportUpload,
	desktopFetch,
	desktopUploadImportFile,
	getMediaProxyBaseUrl,
	getServerConnection,
	initializeMediaProxy,
	listenForQueueEvents,
	listenForServerConnectionChanges,
	releaseDesktopImportSelections,
	saveServerConnection,
	selectDesktopImportFiles,
	selectDesktopImportFolder,
	testServerConnection,
} from "./bridge";

describe("desktop bridge", () => {
	beforeEach(() => {
		invokeMock.mockReset();
		listenMock.mockReset();
		listenMock.mockReturnValue(vi.fn());
	});

	it("exposes renderer connection commands", async () => {
		invokeMock.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});

		await getServerConnection();
		await testServerConnection("http://127.0.0.1:8090");
		await saveServerConnection("http://127.0.0.1:8090");

		expect(invokeMock).toHaveBeenNthCalledWith(1, "get_server_connection");
		expect(invokeMock).toHaveBeenNthCalledWith(2, "test_server_connection", {
			origin: "http://127.0.0.1:8090",
		});
		expect(invokeMock).toHaveBeenNthCalledWith(3, "save_server_connection", {
			origin: "http://127.0.0.1:8090",
		});
	});

	it("selects native files and recursive folders without exposing paths", async () => {
		invokeMock
			.mockResolvedValueOnce([{ id: "opaque-1", name: "song.flac", size: 42 }])
			.mockResolvedValueOnce([{ id: "opaque-2", name: "track.mp3", size: 84 }]);

		await expect(selectDesktopImportFiles()).resolves.toEqual([
			{ id: "opaque-1", name: "song.flac", size: 42 },
		]);
		await expect(selectDesktopImportFolder()).resolves.toEqual([
			{ id: "opaque-2", name: "track.mp3", size: 84 },
		]);
		expect(invokeMock).toHaveBeenNthCalledWith(
			1,
			"desktop_select_import_files",
		);
		expect(invokeMock).toHaveBeenNthCalledWith(
			2,
			"desktop_select_import_folder",
		);
	});

	it("releases opaque native selections", async () => {
		invokeMock.mockResolvedValue(undefined);

		await releaseDesktopImportSelections(["opaque-1", "opaque-2"]);

		expect(invokeMock).toHaveBeenCalledWith(
			"desktop_release_import_selections",
			{
				selectionIds: ["opaque-1", "opaque-2"],
			},
		);
	});

	it("uploads an opaque native selection with progress and cancellation", async () => {
		const progressListeners: Array<(payload: unknown) => void> = [];
		const stopProgress = vi.fn();
		listenMock.mockImplementation(
			(name: string, listener: (payload: unknown) => void) => {
				if (name === "desktop-import-progress")
					progressListeners.push(listener);
				return stopProgress;
			},
		);
		invokeMock.mockImplementation(async (command, args) => {
			if (command === "desktop_upload_import_file") {
				for (const listener of progressListeners) {
					// Progress of another upload is not this one's.
					listener({ uploadId: "other", sentBytes: 9, totalBytes: 10 });
					listener({ uploadId: args.uploadId, sentBytes: 5, totalBytes: 10 });
				}
				return {
					status: 200,
					headers: { "content-type": "application/json" },
					body: Array.from(new TextEncoder().encode('{"jobId":"job-1"}')),
				};
			}
		});
		const onProgress = vi.fn();
		const controller = new AbortController();

		const response = await desktopUploadImportFile(
			"selection-1",
			"job-1",
			onProgress,
			controller.signal,
		);
		expect(await response.json()).toEqual({ jobId: "job-1" });
		expect(onProgress).toHaveBeenCalledWith(50, 5);
		expect(onProgress).not.toHaveBeenCalledWith(90, 9);
		expect(stopProgress).toHaveBeenCalledOnce();
		const uploadCall = invokeMock.mock.calls.find(
			([command]) => command === "desktop_upload_import_file",
		);
		expect(uploadCall?.[1]).toMatchObject({
			selectionId: "selection-1",
			jobId: "job-1",
		});
		expect(uploadCall?.[1]).not.toHaveProperty("url");

		await cancelDesktopImportUpload("upload-2");
		expect(invokeMock).toHaveBeenCalledWith("desktop_cancel_import_upload", {
			uploadId: "upload-2",
		});
	});

	it("cancels an active native upload when its signal aborts", async () => {
		let finishUpload: (() => void) | undefined;
		invokeMock.mockImplementation((command) => {
			if (command === "desktop_upload_import_file") {
				return new Promise((resolve) => {
					finishUpload = () =>
						resolve({
							status: 200,
							headers: {},
							body: Array.from(new TextEncoder().encode("{}")),
						});
				});
			}
			return Promise.resolve();
		});
		const controller = new AbortController();
		const upload = desktopUploadImportFile(
			"selection-1",
			"job-1",
			undefined,
			controller.signal,
		);

		controller.abort();
		await vi.waitFor(() =>
			expect(invokeMock).toHaveBeenCalledWith(
				"desktop_cancel_import_upload",
				expect.objectContaining({ uploadId: expect.any(String) }),
			),
		);
		finishUpload?.();
		await expect(upload).rejects.toMatchObject({ name: "AbortError" });
	});

	it("adapts desktop HTTP responses to fetch", async () => {
		invokeMock.mockResolvedValue({
			status: 200,
			headers: { "content-type": "application/json" },
			body: Array.from(new TextEncoder().encode('{"status":"ok"}')),
		});

		const response = await desktopFetch("/api/v1/health", {
			headers: { Accept: "application/json" },
		});

		await expect(response.json()).resolves.toEqual({ status: "ok" });
		expect(invokeMock).toHaveBeenCalledWith("desktop_http_request", {
			request: {
				method: "GET",
				url: "/api/v1/health",
				headers: { accept: "application/json" },
				body: null,
			},
		});
	});

	it("cancels the core's HTTP request when the caller aborts", async () => {
		let finishRequest: ((value: unknown) => void) | undefined;
		invokeMock.mockImplementation((command: string) =>
			command === "desktop_http_request"
				? new Promise((resolve) => {
						finishRequest = resolve;
					})
				: Promise.resolve(null),
		);
		const controller = new AbortController();

		const response = desktopFetch("/api/v1/external/search?q=a", {
			signal: controller.signal,
		});
		await vi.waitFor(() =>
			expect(invokeMock).toHaveBeenCalledWith(
				"desktop_http_request",
				expect.objectContaining({ requestId: expect.any(String) }),
			),
		);
		const { requestId } = invokeMock.mock.calls[0][1] as {
			requestId: string;
		};
		controller.abort();

		await expect(response).rejects.toMatchObject({ name: "AbortError" });
		expect(invokeMock).toHaveBeenCalledWith("desktop_cancel_http_request", {
			requestId,
		});
		finishRequest?.({ status: 200, headers: {}, body: [] });
	});

	it("does not send a request whose signal is already aborted", async () => {
		const controller = new AbortController();
		controller.abort();

		await expect(
			desktopFetch("/api/v1/health", { signal: controller.signal }),
		).rejects.toMatchObject({ name: "AbortError" });
		expect(invokeMock).not.toHaveBeenCalled();
	});

	it("does not cancel a request that already finished", async () => {
		invokeMock.mockResolvedValue({ status: 204, headers: {}, body: [] });
		const controller = new AbortController();

		await desktopFetch("/api/v1/health", { signal: controller.signal });
		controller.abort();

		expect(invokeMock).toHaveBeenCalledTimes(1);
		expect(invokeMock).toHaveBeenCalledWith("desktop_http_request", {
			request: expect.objectContaining({ url: "/api/v1/health" }),
			requestId: expect.any(String),
		});
	});

	it("sends the body of a Request object", async () => {
		invokeMock.mockResolvedValue({ status: 204, headers: {}, body: [] });

		await desktopFetch(
			new Request("http://127.0.0.1:8090/api/v1/playlists", {
				method: "POST",
				body: "hi",
			}),
		);

		expect(invokeMock).toHaveBeenCalledWith("desktop_http_request", {
			request: expect.objectContaining({
				method: "POST",
				body: Array.from(new TextEncoder().encode("hi")),
			}),
			requestId: expect.any(String),
		});
	});

	it("adapts bodyless desktop HTTP responses", async () => {
		invokeMock.mockResolvedValue({ status: 204, headers: {}, body: [] });

		const response = await desktopFetch("/api/v1/radio/stations/station-1", {
			method: "DELETE",
		});

		expect(response.status).toBe(204);
		expect(await response.text()).toBe("");
	});

	it("subscribes to the server connection event", async () => {
		const callback = vi.fn();

		await listenForServerConnectionChanges(callback);

		expect(listenMock).toHaveBeenCalledWith(
			"server-connection-changed",
			expect.any(Function),
		);
	});

	it("subscribes to native Queue events and errors", async () => {
		const listeners = new Map<string, (payload: unknown) => void>();
		const unlistenQueue = vi.fn();
		const unlistenError = vi.fn();
		listenMock.mockImplementation(
			(name: string, callback: (payload: unknown) => void) => {
				listeners.set(name, callback);
				return name === "desktop-queue-invalidated"
					? unlistenQueue
					: unlistenError;
			},
		);
		const onEvent = vi.fn();
		const onError = vi.fn();

		const unsubscribe = await listenForQueueEvents(onEvent, onError);
		expect(invokeMock).toHaveBeenCalledWith("desktop_reconnect_queue_events");
		listeners.get("desktop-queue-invalidated")?.({
			revision: "opaque-2",
			sequence: "2",
			invalidates: ["queue"],
		});
		listeners.get("desktop-queue-events-error")?.("connection lost");

		expect(onEvent).toHaveBeenCalledWith({
			revision: "opaque-2",
			sequence: "2",
			invalidates: ["queue"],
		});
		expect(onError).toHaveBeenCalledWith(
			new Error("Desktop Queue event stream: connection lost"),
		);
		unsubscribe();
		expect(unlistenQueue).toHaveBeenCalledOnce();
		expect(unlistenError).toHaveBeenCalledOnce();
	});

	it("initializes the tokenized loopback media proxy URL", async () => {
		const proxyUrl = `http://127.0.0.1:41000/${"a".repeat(64)}`;
		invokeMock.mockResolvedValue(proxyUrl);

		await expect(initializeMediaProxy()).resolves.toBe(proxyUrl);
		expect(getMediaProxyBaseUrl()).toBe(proxyUrl);
		expect(invokeMock).toHaveBeenCalledWith("get_media_proxy_url");
	});
});
