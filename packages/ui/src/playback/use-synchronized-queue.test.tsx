import {
	ApiError,
	type Queue,
	type QueueEvent,
	type QueueItemSource,
} from "@repo/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";
import {
	type PlaybackQueueApi,
	useSynchronizedQueue,
} from "./use-synchronized-queue";

const firstQueue: Queue = {
	items: [
		{
			id: "item-1",
			trackId: "track-1",
			position: 0,
			track: {
				id: "track-1",
				title: "First",
				artistName: "Artist",
				artists: [],
				albumId: "album-1",
				discNo: 1,
				durationMs: 1000,
				format: "flac",
				genres: [],
			},
		},
	],
	revision: "opaque-1",
};

const latestQueue: Queue = {
	items: [
		{
			id: "item-2",
			trackId: "track-2",
			position: 0,
			track: {
				id: "track-2",
				title: "Latest",
				artistName: "Artist",
				artists: [],
				albumId: "album-1",
				discNo: 1,
				durationMs: 1000,
				format: "flac",
				genres: [],
			},
		},
	],
	revision: "opaque-3",
};

describe("useSynchronizedQueue", () => {
	it.each(["append", "replace"] as const)(
		"preserves source across a revision conflict on %s",
		async (operation) => {
			const api = createQueueApi();
			const source: QueueItemSource = {
				kind: "album",
				albumId: "album-1",
				albumTitle: "Album",
				artistName: "Artist",
			};
			vi.mocked(api.getQueue)
				.mockResolvedValueOnce(firstQueue)
				.mockResolvedValueOnce({ ...firstQueue, revision: "fresh" });
			const mutate =
				operation === "append" ? api.appendQueueItem : api.replaceQueue;
			vi.mocked(mutate)
				.mockRejectedValueOnce(
					new ApiError(409, {
						error: "Queue changed",
						code: "queue_revision_conflict",
						message: "Queue changed",
					}),
				)
				.mockResolvedValueOnce(latestQueue);
			const { result } = renderHook(() => useSynchronizedQueue(api));
			await waitFor(() =>
				expect(result.current.queue).toEqual(firstQueue.items),
			);
			await act(async () => {
				if (operation === "append")
					await result.current.appendQueueItem("track-2", source);
				else
					await result.current.replaceQueue(["track-2"], {
						retryOnConflict: true,
						source,
					});
			});
			const tracks = operation === "append" ? "track-2" : ["track-2"];
			expect(mutate).toHaveBeenNthCalledWith(
				1,
				tracks,
				firstQueue.revision,
				source,
			);
			expect(mutate).toHaveBeenNthCalledWith(2, tracks, "fresh", source);
			expect(result.current.queue).toEqual(latestQueue.items);
		},
	);

	it("refetches after revision gaps and ignores duplicate or older events", async () => {
		let notifyQueueEvent: ((event: QueueEvent) => void) | undefined;
		const unsubscribe = vi.fn();
		const api = createQueueApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce(firstQueue)
			.mockResolvedValueOnce(latestQueue);
		api.subscribeQueueEvents = vi.fn((listener) => {
			notifyQueueEvent = listener;
			return unsubscribe;
		});

		const { result, unmount } = renderHook(() => useSynchronizedQueue(api));
		await waitFor(() => expect(result.current.queue).toEqual(firstQueue.items));

		act(() => {
			notifyQueueEvent?.({
				revision: "opaque-1",
				sequence: "1",
				invalidates: ["queue"],
			});
			notifyQueueEvent?.({
				revision: "opaque-0",
				sequence: "0",
				invalidates: ["queue"],
			});
		});
		expect(api.getQueue).toHaveBeenCalledOnce();

		act(() => {
			notifyQueueEvent?.({
				revision: "opaque-3",
				sequence: "3",
				invalidates: ["queue"],
			});
		});
		await waitFor(() =>
			expect(result.current.queue).toEqual(latestQueue.items),
		);
		expect(api.getQueue).toHaveBeenCalledTimes(2);

		unmount();
		expect(unsubscribe).toHaveBeenCalledOnce();
	});

	it("keeps newest Queue when refetches resolve out of revision order", async () => {
		let notifyQueueEvent: ((event: QueueEvent) => void) | undefined;
		const staleResponse = deferred<Queue>();
		const latestResponse = deferred<Queue>();
		const api = createQueueApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce(firstQueue)
			.mockImplementationOnce(() => staleResponse.promise)
			.mockImplementationOnce(() => latestResponse.promise);
		api.subscribeQueueEvents = vi.fn((listener) => {
			notifyQueueEvent = listener;
			return vi.fn();
		});
		const { result } = renderHook(() => useSynchronizedQueue(api));
		await waitFor(() => expect(result.current.queue).toEqual(firstQueue.items));

		act(() => {
			notifyQueueEvent?.({
				revision: "opaque-3",
				sequence: "3",
				invalidates: ["queue"],
			});
			notifyQueueEvent?.({
				revision: "opaque-4",
				sequence: "4",
				invalidates: ["queue"],
			});
		});
		await act(async () => {
			latestResponse.resolve({ ...latestQueue, revision: "opaque-4" });
		});
		await waitFor(() =>
			expect(result.current.queue).toEqual(latestQueue.items),
		);

		await act(async () => {
			staleResponse.resolve({ ...firstQueue, revision: "opaque-3" });
		});
		expect(result.current.queue).toEqual(latestQueue.items);
	});

	it("retries a replayed event when its previous refetch failed", async () => {
		let notifyQueueEvent: ((event: QueueEvent) => void) | undefined;
		const api = createQueueApi();
		vi.mocked(api.getQueue)
			.mockResolvedValueOnce(firstQueue)
			.mockRejectedValueOnce(new Error("temporary disconnect"))
			.mockResolvedValueOnce(latestQueue);
		api.subscribeQueueEvents = vi.fn((listener) => {
			notifyQueueEvent = listener;
			return vi.fn();
		});
		const { result } = renderHook(() => useSynchronizedQueue(api));
		await waitFor(() => expect(result.current.queue).toEqual(firstQueue.items));

		await act(async () => {
			notifyQueueEvent?.({
				revision: "opaque-3",
				sequence: "3",
				invalidates: ["queue"],
			});
		});
		expect(result.current.queue).toEqual(firstQueue.items);

		await act(async () => {
			notifyQueueEvent?.({
				revision: "opaque-3",
				sequence: "3",
				invalidates: ["queue"],
			});
		});
		await waitFor(() =>
			expect(result.current.queue).toEqual(latestQueue.items),
		);
		expect(api.getQueue).toHaveBeenCalledTimes(3);
	});
});

function createQueueApi(): PlaybackQueueApi {
	return {
		getQueue: vi.fn(),
		replaceQueue: vi.fn(),
		reorderQueue: vi.fn(),
		appendQueueItem: vi.fn(),
		removeQueueItem: vi.fn(),
		clearQueue: vi.fn(),
	};
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	const promise = new Promise<T>((complete) => {
		resolve = complete;
	});
	return { promise, resolve };
}
