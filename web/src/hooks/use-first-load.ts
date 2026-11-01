import { useState } from "react";

/**
 * True until `isLoading` first goes false, and never again. A query without
 * data goes back to loading on every refetch, which must not unmount the app.
 */
export function useFirstLoad(isLoading: boolean): boolean {
	const [hasLoaded, setHasLoaded] = useState(false);
	if (!isLoading && !hasLoaded) setHasLoaded(true);
	return isLoading && !hasLoaded;
}
