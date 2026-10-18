const MAX_CONCURRENT_UPLOADS = 2;
const RETRY_DELAYS_MS = [2000, 5000, 10000];

export function waitForImport(
	delayMs: number,
	signal?: AbortSignal,
): Promise<void> {
	return new Promise((resolve, reject) => {
		const handleAbort = () => {
			clearTimeout(timer);
			signal?.removeEventListener("abort", handleAbort);
			reject(
				signal?.reason ?? new DOMException("Import canceled", "AbortError"),
			);
		};
		const timer = setTimeout(() => {
			signal?.removeEventListener("abort", handleAbort);
			resolve();
		}, delayMs);
		signal?.addEventListener("abort", handleAbort, { once: true });
		if (signal?.aborted) handleAbort();
	});
}

export async function runImportUploads<T>(
	items: T[],
	attempt: (item: T, isRetry: boolean) => Promise<boolean>,
	onRetry: (item: T, retryCount: number, retryAt: number) => void,
	signal?: AbortSignal,
) {
	let activeCount = 0;
	const waiting: Array<() => void> = [];
	async function runAttempt(item: T, isRetry: boolean) {
		if (activeCount >= MAX_CONCURRENT_UPLOADS) {
			await new Promise<void>((resolve) => waiting.push(resolve));
		} else activeCount += 1;
		try {
			signal?.throwIfAborted();
			return await attempt(item, isRetry);
		} finally {
			const next = waiting.shift();
			if (next) next();
			else activeCount -= 1;
		}
	}
	async function runFile(item: T) {
		for (
			let attemptIndex = 0;
			attemptIndex <= RETRY_DELAYS_MS.length;
			attemptIndex++
		) {
			if (attemptIndex > 0) {
				const delayMs = RETRY_DELAYS_MS[attemptIndex - 1] ?? 0;
				onRetry(item, attemptIndex, Date.now() + delayMs);
				await waitForImport(delayMs, signal);
			}
			if (!(await runAttempt(item, attemptIndex > 0))) return;
		}
	}
	const results = await Promise.allSettled(items.map(runFile));
	for (const result of results) {
		if (result.status === "rejected") throw result.reason;
	}
}
