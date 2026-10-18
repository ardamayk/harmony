import { useEffect, useState } from "react";

/** Returns `value` once it has stayed unchanged for `delayMs`. */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
	const [debounced, setDebounced] = useState(value);

	useEffect(() => {
		const timeout = window.setTimeout(() => setDebounced(value), delayMs);
		return () => window.clearTimeout(timeout);
	}, [value, delayMs]);

	return debounced;
}
