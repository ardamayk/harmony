import { ApiError, type ManagedImportBatch } from "@repo/api-client";
import { cleanup, renderHook } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { apiClient } from "#/lib/api";
import { useImportSessionLifecycle } from "./-import-session-lifecycle";

vi.mock("#/lib/api", () => ({
	apiClient: {
		getManagedImportBatch: vi.fn(),
		heartbeatManagedImportBatch: vi.fn(),
	},
}));

const getBatch = vi.mocked(apiClient.getManagedImportBatch);
const heartbeat = vi.mocked(apiClient.heartbeatManagedImportBatch);

function batch(status: ManagedImportBatch["status"]): ManagedImportBatch {
	return { id: "batch-1", status, revision: 1, files: [], albums: [] };
}

function observe(initial: ManagedImportBatch) {
	const onError = vi.fn();
	const hook = renderHook(
		(current: ManagedImportBatch) =>
			useImportSessionLifecycle({
				batch: current,
				isProcessing: true,
				onBatch: async () => {},
				onError,
				onExit: () => {},
			}),
		{ initialProps: initial },
	);
	return { ...hook, onError };
}

describe("useImportSessionLifecycle", () => {
	beforeEach(() => {
		vi.useFakeTimers();
		heartbeat.mockResolvedValue(undefined);
	});
	afterEach(() => {
		cleanup();
		vi.useRealTimers();
		vi.resetAllMocks();
	});

	it("stops polling once the Batch is completed", async () => {
		getBatch.mockResolvedValue(batch("confirming"));
		const { rerender } = observe(batch("confirming"));
		await vi.advanceTimersByTimeAsync(1000);
		expect(getBatch).toHaveBeenCalledTimes(1);

		rerender(batch("completed"));
		await vi.advanceTimersByTimeAsync(5000);
		expect(getBatch).toHaveBeenCalledTimes(1);
	});

	it("stops polling and heartbeats once the Batch is gone", async () => {
		const gone = new ApiError(404, {
			error: "not_found",
			code: "not_found",
			message: "Managed Import Batch not found",
		});
		getBatch.mockRejectedValue(gone);
		const { onError } = observe(batch("uploading"));
		await vi.advanceTimersByTimeAsync(1000);
		expect(getBatch).toHaveBeenCalledTimes(1);
		expect(onError).toHaveBeenCalledWith(gone);

		await vi.advanceTimersByTimeAsync(60_000);
		expect(getBatch).toHaveBeenCalledTimes(1);
		expect(heartbeat).toHaveBeenCalledTimes(1);
	});
});
