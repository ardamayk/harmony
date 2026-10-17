import { type RefObject, useLayoutEffect, useState } from "react";

// Match the fixed collection cards and gap used by the host's collection grid.
const COLLECTION_CARD_WIDTH_PX = 250;
const COLLECTION_GRID_GAP_REM = 0.75;
const MAX_COLLECTION_COLUMNS = 6;
const QUEUE_MIN_WIDTH_REM = 18;
const QUEUE_GAP_REM = 1.5;
// Below Tailwind's `lg` (1024px) the drawer has a fixed width, narrower again
// below `md` (768px), and the collection grid stretches two columns into the
// rest. Mirrored by the `max-lg` grid class in the Web Client's
// collection-grid-layout.
const LG_BREAKPOINT_PX = 1024;
const MD_BREAKPOINT_PX = 768;
const QUEUE_BELOW_LG_WIDTH_PX = 386;
const QUEUE_BELOW_MD_WIDTH_PX = 275;

export function calculateQueueDrawerWidth(
	shellWidth: number,
	inset: number,
	rootFontSize: number,
): number {
	const minimumWidth = QUEUE_MIN_WIDTH_REM * rootFontSize;
	const availableWidth = Math.max(0, shellWidth - 2 * inset);
	if (shellWidth < MD_BREAKPOINT_PX)
		return Math.min(QUEUE_BELOW_MD_WIDTH_PX, availableWidth);
	if (shellWidth < LG_BREAKPOINT_PX)
		return Math.min(QUEUE_BELOW_LG_WIDTH_PX, availableWidth);
	// From `lg` the drawer fills whatever the fixed cards leave over, so its
	// left edge always sits one gap from the last card.
	const margin = QUEUE_GAP_REM * rootFontSize;
	const collectionGap = COLLECTION_GRID_GAP_REM * rootFontSize;
	const columns = Math.min(
		MAX_COLLECTION_COLUMNS,
		Math.floor(
			(availableWidth - minimumWidth - margin + collectionGap) /
				(COLLECTION_CARD_WIDTH_PX + collectionGap),
		),
	);
	if (columns < 1) return Math.min(minimumWidth, availableWidth);
	const collectionWidth =
		columns * COLLECTION_CARD_WIDTH_PX + (columns - 1) * collectionGap;
	return availableWidth - collectionWidth - margin;
}

export function calculateLibrarySearchBounds(
	shellWidth: number,
	inset: number,
	rootFontSize: number,
) {
	const gap = COLLECTION_GRID_GAP_REM * rootFontSize;
	const pitch = COLLECTION_CARD_WIDTH_PX + gap;
	const columns = Math.floor(
		(Math.max(0, shellWidth - 2 * inset) + gap) / pitch,
	);
	// Never narrower than 52rem (or the window less gutters): between the third
	// cards' centers is one card wide at six columns.
	const minimumWidth = Math.max(
		0,
		Math.min(52 * rootFontSize, shellWidth - 32),
	);
	if (columns < 6) return { width: minimumWidth, center: shellWidth / 2 };
	return {
		width: Math.max(minimumWidth, (columns - 5) * pitch),
		center: inset + (columns * pitch - gap) / 2,
	};
}

/** Measure Queue and Search from the full shell, independently of Queue visibility. */
export function useQueueDrawerWidth(
	shellRef: RefObject<HTMLDivElement | null>,
) {
	const [metrics, setMetrics] = useState<{
		queueWidth: number;
		searchWidth: number;
		searchCenter: number;
	}>();
	useLayoutEffect(() => {
		const shell = shellRef.current;
		const navigation = shell?.querySelector("header");
		if (!shell || !navigation) return;
		const measure = () => {
			const shellWidth = shell.getBoundingClientRect().width;
			if (shellWidth === 0) return;
			// The left padding is always the shell inset; the right one is not
			// when the Top Nav carries window controls at the window's corner.
			const inset =
				Number.parseFloat(getComputedStyle(navigation).paddingLeft) || 0;
			const rootFontSize =
				Number.parseFloat(
					getComputedStyle(document.documentElement).fontSize,
				) || 16;
			const search = calculateLibrarySearchBounds(
				shellWidth,
				inset,
				rootFontSize,
			);
			setMetrics({
				queueWidth: calculateQueueDrawerWidth(shellWidth, inset, rootFontSize),
				searchWidth: search.width,
				searchCenter: shell.getBoundingClientRect().left + search.center,
			});
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(shell);
		window.addEventListener("resize", measure);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", measure);
		};
	}, [shellRef]);
	return metrics;
}
