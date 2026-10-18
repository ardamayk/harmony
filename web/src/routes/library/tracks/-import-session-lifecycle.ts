import type { ManagedImportBatch } from "@repo/api-client";
import { useEffect, useRef } from "react";
import { apiClient } from "#/lib/api";

const POLL_INTERVAL_MS = 1000;
const HEARTBEAT_INTERVAL_MS = 30000;

type ImportSessionObserver = {
	batch?: ManagedImportBatch;
	isProcessing: boolean;
	onBatch: (batch: ManagedImportBatch) => Promise<void>;
	onError: (error: unknown) => void;
	onExit: () => void;
};

export function useImportSessionLifecycle(observer: ImportSessionObserver) {
	const latest = useRef(observer);
	useEffect(() => {
		latest.current = observer;
	});
	const batchId = observer.batch?.id;
	const isCompleted = observer.batch?.status === "completed";
	useEffect(() => {
		if (!batchId || isCompleted) return;
		return observeImport(batchId, () => latest.current);
	}, [batchId, isCompleted]);
	useEffect(() => {
		const handleExit = () => latest.current.onExit();
		window.addEventListener("pagehide", handleExit);
		return () => {
			window.removeEventListener("pagehide", handleExit);
			handleExit();
		};
	}, []);
}

function observeImport(batchId: string, current: () => ImportSessionObserver) {
	let isDisposed = false;
	let isPolling = false;
	let lastHeartbeat = 0;
	let isHeartbeating = false;
	async function heartbeat() {
		if (isHeartbeating || Date.now() - lastHeartbeat < HEARTBEAT_INTERVAL_MS)
			return;
		isHeartbeating = true;
		lastHeartbeat = Date.now();
		try {
			await apiClient.heartbeatManagedImportBatch(batchId);
		} catch (error) {
			if (!isDisposed) current().onError(error);
		} finally {
			isHeartbeating = false;
		}
	}
	async function poll() {
		if (isPolling || isDisposed) return;
		isPolling = true;
		try {
			void heartbeat();
			if (current().isProcessing || current().batch?.status === "confirming") {
				const batch = await apiClient.getManagedImportBatch(batchId);
				if (!isDisposed) await current().onBatch(batch);
			}
		} catch (error) {
			if (!isDisposed) current().onError(error);
		} finally {
			isPolling = false;
		}
	}
	const timer = setInterval(() => void poll(), POLL_INTERVAL_MS);
	return () => {
		isDisposed = true;
		clearInterval(timer);
	};
}
