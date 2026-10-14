import { useEffect, useState } from "react";
import {
	type Accent,
	type AccentThemeMode,
	accentCssVariables,
	extractAccent,
} from "../lib/cover-accent";

const accentCache = new Map<string, Accent | null>();

/** Test hook: forget every cached accent. */
export function clearCoverAccentCache() {
	accentCache.clear();
}

/**
 * Resolves the accent for a cover URL, cached per key (the album id) so a
 * track change within an album never flickers. Returns CSS custom
 * properties for the bar root, or an empty object while unknown/disabled.
 */
export function useCoverAccent({
	cacheKey,
	coverUrl,
	enabled,
	mode,
	loadImage = defaultLoadImage,
}: {
	cacheKey: string | null;
	coverUrl: string | null;
	enabled: boolean;
	mode: AccentThemeMode;
	loadImage?: (url: string) => Promise<HTMLImageElement>;
}): Record<string, string> {
	// The last accent shown; kept while an uncached cover loads so moving to
	// a new album does not flash the default colors.
	const [shown, setShown] = useState<Accent | null>(null);
	const activeKey = enabled && cacheKey && coverUrl ? cacheKey : null;
	const isCached = activeKey !== null && accentCache.has(activeKey);
	const accent = isCached
		? (accentCache.get(activeKey) ?? null)
		: activeKey
			? shown
			: null;
	if (accent !== shown) setShown(accent);

	useEffect(() => {
		if (!activeKey || !coverUrl || accentCache.has(activeKey)) return undefined;
		let cancelled = false;
		loadImage(coverUrl)
			.then((image) => {
				const extracted = extractAccent(image);
				accentCache.set(activeKey, extracted);
				if (!cancelled) setShown(extracted);
			})
			.catch(() => {
				accentCache.set(activeKey, null);
				if (!cancelled) setShown(null);
			});
		return () => {
			cancelled = true;
		};
	}, [activeKey, coverUrl, loadImage]);

	return accentCssVariables(accent, mode);
}

function defaultLoadImage(url: string): Promise<HTMLImageElement> {
	return new Promise((resolve, reject) => {
		const image = new Image();
		// Needed for an untainted canvas; the server and the desktop cover
		// scheme both answer with CORS headers.
		image.crossOrigin = "anonymous";
		image.onload = () => resolve(image);
		image.onerror = () => reject(new Error("cover could not be loaded"));
		image.src = url;
	});
}
