type ViteEnv = { DEV?: boolean; MODE?: string };

// Not every consumer has Vite's types; Vite replaces `import.meta.env`.
const env = (import.meta as ImportMeta & { env?: ViteEnv }).env;
const shouldLog = env?.DEV === true && env.MODE !== "test";
let nextMark = 0;

/** Ends a timing; only its first call counts. */
export type StopTiming = (detail?: Record<string, number>) => void;

function hasUserTiming(): boolean {
	return (
		typeof performance !== "undefined" &&
		typeof performance.mark === "function" &&
		typeof performance.measure === "function"
	);
}

/**
 * Times something until the returned function is called: a
 * `performance.mark` now, a `performance.measure` named `name` at the end
 * (shown in the browser's performance tools), and in development the
 * milliseconds on the console. Names and numbers only, never URLs or
 * queries.
 */
export function startTiming(name: string): StopTiming {
	if (!hasUserTiming()) return () => {};
	nextMark += 1;
	const mark = `${name}:start:${nextMark}`;
	try {
		performance.mark(mark);
	} catch {
		return () => {};
	}
	let isStopped = false;
	return (detail) => {
		if (isStopped) return;
		isStopped = true;
		try {
			const measure = performance.measure(name, mark);
			performance.clearMarks(mark);
			if (shouldLog) {
				console.info(
					`[timing] ${name} ${Math.round(measure.duration)} ms`,
					...(detail ? [detail] : []),
				);
			}
		} catch {
			// The start mark was cleared elsewhere; nothing to report.
		}
	};
}
