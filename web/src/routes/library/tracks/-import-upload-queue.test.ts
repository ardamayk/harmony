import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { runImportUploads } from "./-import-upload-queue";

describe("runImportUploads", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("runs a retry before the files still waiting for their first try", async () => {
		const started: string[] = [];
		const finish = new Map<string, (retry: boolean) => void>();
		const run = runImportUploads(
			["a", "b", "c", "d"],
			(item, isRetry) => {
				const attempt = isRetry ? `${item} retry` : item;
				started.push(attempt);
				return new Promise((resolve) => finish.set(attempt, resolve));
			},
			vi.fn(),
		);
		await vi.advanceTimersByTimeAsync(0);
		expect(started).toEqual(["a", "b"]);

		finish.get("a")?.(true);
		await vi.advanceTimersByTimeAsync(2000);
		expect(started).toEqual(["a", "b", "c"]);

		finish.get("b")?.(false);
		await vi.advanceTimersByTimeAsync(0);
		expect(started).toEqual(["a", "b", "c", "a retry"]);

		finish.get("c")?.(false);
		finish.get("a retry")?.(false);
		await vi.advanceTimersByTimeAsync(0);
		finish.get("d")?.(false);
		await run;
		expect(started).toEqual(["a", "b", "c", "a retry", "d"]);
	});
});
