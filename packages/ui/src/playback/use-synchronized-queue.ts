import type {
	Queue,
	QueueEvent,
	QueueItem,
	QueueItemSource,
} from "@repo/api-client";
import { ApiError } from "@repo/api-client";
import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";

export type PlaybackQueueApi = {
	getQueue: () => Promise<Queue>;
	replaceQueue: (
		trackIds: string[],
		revision: string,
		source?: QueueItemSource,
	) => Promise<Queue>;
	reorderQueue: (itemIds: string[], revision: string) => Promise<Queue>;
	appendQueueItem: (
		trackId: string,
		revision: string,
		source?: QueueItemSource,
	) => Promise<Queue>;
	removeQueueItem: (itemId: string, revision: string) => Promise<Queue>;
	clearQueue: (afterItemId: string | null, revision: string) => Promise<Queue>;
	subscribeQueueEvents?: (
		onEvent: (event: QueueEvent) => void,
		onError?: (error: Error) => void,
	) => () => void;
};

export function useSynchronizedQueue(api: PlaybackQueueApi) {
	const apiRef = useRef(api);
	const queueRef = useRef<QueueItem[]>([]);
	const queueRevisionRef = useRef("0");
	const latestEventSequenceRef = useRef<bigint | null>(null);
	const pendingEventSequencesRef = useRef(new Set<bigint>());
	const nextQueueRequestRef = useRef(0);
	const latestAppliedRequestRef = useRef(0);
	const [queue, setQueueState] = useState<QueueItem[]>([]);
	const [queueConflict, setQueueConflict] = useState<string | null>(null);
	useLayoutEffect(() => {
		apiRef.current = api;
	});

	const applyQueue = useCallback((data: Queue, requestOrder: number) => {
		if (requestOrder < latestAppliedRequestRef.current) return;
		queueRef.current = data.items;
		queueRevisionRef.current = data.revision;
		latestAppliedRequestRef.current = requestOrder;
		setQueueState(data.items);
	}, []);

	const runQueueRequest = useCallback(
		async (request: () => Promise<Queue>): Promise<Queue> => {
			const requestOrder = ++nextQueueRequestRef.current;
			const data = await request();
			applyQueue(data, requestOrder);
			return data;
		},
		[applyQueue],
	);

	const refreshQueue = useCallback(async () => {
		await runQueueRequest(() => apiRef.current.getQueue());
	}, [runQueueRequest]);

	useEffect(() => {
		void refreshQueue().catch((error) => {
			console.warn("Failed to refresh Queue", { error });
		});
	}, [refreshQueue]);

	useEffect(() => {
		if (!apiRef.current.subscribeQueueEvents) return undefined;
		return apiRef.current.subscribeQueueEvents(
			(event) => {
				const sequence = BigInt(event.sequence);
				if (
					!event.invalidates.includes("queue") ||
					(latestEventSequenceRef.current !== null &&
						sequence <= latestEventSequenceRef.current)
				) {
					return;
				}
				if (pendingEventSequencesRef.current.has(sequence)) return;
				if (event.revision === queueRevisionRef.current) {
					latestEventSequenceRef.current = sequence;
					return;
				}
				pendingEventSequencesRef.current.add(sequence);
				void refreshQueue()
					.then(() => {
						if (
							latestEventSequenceRef.current === null ||
							sequence > latestEventSequenceRef.current
						) {
							latestEventSequenceRef.current = sequence;
						}
					})
					.catch((error) => {
						console.warn("Failed to refresh invalidated Queue", {
							revision: event.revision,
							error,
						});
					})
					.finally(() => {
						pendingEventSequencesRef.current.delete(sequence);
					});
			},
			(error) => {
				console.warn("Queue event stream disconnected", { error });
			},
		);
	}, [refreshQueue]);

	/** Refetch the Queue and re-issue one mutation against the fresh revision. */
	const retryWithFreshRevision = useCallback(
		async (mutate: (revision: string) => Promise<Queue>): Promise<Queue> => {
			const current = await runQueueRequest(() => apiRef.current.getQueue());
			const retried = await runQueueRequest(() => mutate(current.revision));
			setQueueConflict(null);
			return retried;
		},
		[runQueueRequest],
	);

	const appendQueueItem = useCallback(
		async (trackId: string, source?: QueueItemSource): Promise<Queue> => {
			try {
				const data = await runQueueRequest(() =>
					apiRef.current.appendQueueItem(
						trackId,
						queueRevisionRef.current,
						...(source ? [source] : []),
					),
				);
				setQueueConflict(null);
				return data;
			} catch (error) {
				if (!isQueueConflict(error)) throw error;
				return retryWithFreshRevision((revision) =>
					apiRef.current.appendQueueItem(
						trackId,
						revision,
						...(source ? [source] : []),
					),
				);
			}
		},
		[retryWithFreshRevision, runQueueRequest],
	);

	/**
	 * Replace the Queue. A replace whose content does not depend on the
	 * previous Queue (playing a Track with its context) is an unambiguous
	 * intent and may be retried once after a revision conflict; other
	 * replacements surface the conflict for the user to resolve.
	 */
	const replaceQueue = useCallback(
		async (
			trackIds: string[],
			options: { retryOnConflict?: boolean; source?: QueueItemSource } = {},
		): Promise<Queue | undefined> => {
			try {
				const data = await runQueueRequest(() =>
					apiRef.current.replaceQueue(
						trackIds,
						queueRevisionRef.current,
						...(options.source ? [options.source] : []),
					),
				);
				setQueueConflict(null);
				return data;
			} catch (error) {
				if (!isQueueConflict(error)) throw error;
				if (options.retryOnConflict) {
					return retryWithFreshRevision((revision) =>
						apiRef.current.replaceQueue(
							trackIds,
							revision,
							...(options.source ? [options.source] : []),
						),
					);
				}
				await runQueueRequest(() => apiRef.current.getQueue());
				setQueueConflict(
					"Queue changed in another Playback Client. Review replacement and try again.",
				);
				return undefined;
			}
		},
		[retryWithFreshRevision, runQueueRequest],
	);

	const reconcileRemove = useCallback(
		async (itemId: string) => {
			const current = await runQueueRequest(() => apiRef.current.getQueue());
			if (!current.items.some((item) => item.id === itemId)) {
				setQueueConflict(null);
				return;
			}
			await runQueueRequest(() =>
				apiRef.current.removeQueueItem(itemId, current.revision),
			);
			setQueueConflict(null);
		},
		[runQueueRequest],
	);

	const removeFromQueue = useCallback(
		async (itemId: string) => {
			try {
				await runQueueRequest(() =>
					apiRef.current.removeQueueItem(itemId, queueRevisionRef.current),
				);
				setQueueConflict(null);
			} catch (error) {
				if (!isQueueConflict(error)) throw error;
				await reconcileRemove(itemId);
			}
		},
		[reconcileRemove, runQueueRequest],
	);

	/** Removes the items after afterItemId (the playing item); null clears all. */
	const clearQueue = useCallback(
		async (afterItemId: string | null) => {
			try {
				await runQueueRequest(() =>
					apiRef.current.clearQueue(afterItemId, queueRevisionRef.current),
				);
				setQueueConflict(null);
			} catch (error) {
				if (!isQueueConflict(error)) throw error;
				await runQueueRequest(() => apiRef.current.getQueue());
				setQueueConflict(
					"Queue changed in another Playback Client. Review the queue and try again.",
				);
			}
		},
		[runQueueRequest],
	);

	const reorderQueue = useCallback(
		async (itemIds: string[]) => {
			try {
				await runQueueRequest(() =>
					apiRef.current.reorderQueue(itemIds, queueRevisionRef.current),
				);
				setQueueConflict(null);
			} catch (error) {
				if (!isQueueConflict(error)) throw error;
				await runQueueRequest(() => apiRef.current.getQueue());
				setQueueConflict(
					"Queue changed in another Playback Client. Review order and try again.",
				);
			}
		},
		[runQueueRequest],
	);

	return {
		queue,
		queueRef,
		queueConflict,
		refreshQueue,
		appendQueueItem,
		replaceQueue,
		removeFromQueue,
		clearQueue,
		reorderQueue,
	};
}

function isQueueConflict(error: unknown): error is ApiError {
	return (
		error instanceof ApiError &&
		error.status === 409 &&
		error.body.code === "queue_revision_conflict"
	);
}
