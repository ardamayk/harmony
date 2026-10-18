import { type RefObject, useLayoutEffect, useState } from "react";

// Match the fixed collection cards and gap used by the host's collection grid.
const COLLECTION_CARD_WIDTH_PX = 250;
const COLLECTION_GRID_GAP_REM = 0.75;
const MAX_COLLECTION_COLUMNS = 6;
const QUEUE_MIN_WIDTH_REM = 18;
// Below Tailwind's `lg` (1024px) the drawer has a fixed width, narrower again
// below `md` (768px), and the collection grid stretches two columns into the
// rest. Mirrored by the `max-lg` grid class in the Web Client's
// collection-grid-layout.
const LG_BREAKPOINT_PX = 1024;
const MD_BREAKPOINT_PX = 768;
const QUEUE_BELOW_LG_WIDTH_PX = 386;
const QUEUE_BELOW_MD_WIDTH_PX = 275;

/**
 * `shellWidth` decides the breakpoints (it is the window's width); the cards
 * and the drawer share what the Sidebar and the page's two insets leave.
 */
export function calculateQueueDrawerWidth(
	shellWidth: number,
	inset: number,
	rootFontSize: number,
	sidebarWidth = 0,
): number {
	const minimumWidth = QUEUE_MIN_WIDTH_REM * rootFontSize;
	const availableWidth = Math.max(0, shellWidth - sidebarWidth - 2 * inset);
	if (shellWidth < MD_BREAKPOINT_PX)
		return Math.min(QUEUE_BELOW_MD_WIDTH_PX, availableWidth);
	if (shellWidth < LG_BREAKPOINT_PX)
		return Math.min(QUEUE_BELOW_LG_WIDTH_PX, availableWidth);
	// From `lg` the docked drawer fills whatever the fixed cards leave over,
	// so its left edge sits one page inset from the last card.
	const collectionGap = COLLECTION_GRID_GAP_REM * rootFontSize;
	const columns = Math.min(
		MAX_COLLECTION_COLUMNS,
		Math.floor(
			(availableWidth - minimumWidth + collectionGap) /
				(COLLECTION_CARD_WIDTH_PX + collectionGap),
		),
	);
	if (columns < 1) return Math.min(minimumWidth, availableWidth);
	const collectionWidth =
		columns * COLLECTION_CARD_WIDTH_PX + (columns - 1) * collectionGap;
	return availableWidth - collectionWidth;
}

/**
 * Measure the Queue Drawer from the shell, independently of Queue
 * visibility, and where the search card drops down: over the top bar's
 * search field, as wide as it.
 */
export function useQueueDrawerWidth(
	shellRef: RefObject<HTMLDivElement | null>,
) {
	const [metrics, setMetrics] = useState<{
		queueWidth: number;
		searchWidth: number;
		searchCenter: number;
		searchTop: number;
	}>();
	useLayoutEffect(() => {
		const shell = shellRef.current;
		if (!shell) return;
		const measure = () => {
			const shellWidth = shell.getBoundingClientRect().width;
			if (shellWidth === 0) return;
			// The page inset is a CSS clamp; a probe sized by it resolves it.
			const inset =
				shell.querySelector<HTMLElement>("[data-shell-inset-probe]")
					?.offsetWidth ?? 0;
			const sidebarWidth =
				shell.querySelector<HTMLElement>("[data-sidebar]")?.offsetWidth ?? 0;
			const rootFontSize =
				Number.parseFloat(
					getComputedStyle(document.documentElement).fontSize,
				) || 16;
			const field = shell
				.querySelector("[data-library-search-field]")
				?.getBoundingClientRect();
			setMetrics({
				queueWidth: calculateQueueDrawerWidth(
					shellWidth,
					inset,
					rootFontSize,
					sidebarWidth,
				),
				searchWidth: field?.width ?? 0,
				searchCenter: field ? field.left + field.width / 2 : 0,
				searchTop: field?.top ?? 0,
			});
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(shell);
		// The field moves and resizes without the shell resizing (the Home
		// button or brand beside it changing size, a late web font), and the
		// search card must still drop exactly over it.
		const field = shell.querySelector("[data-library-search-field]");
		if (field) observer.observe(field);
		window.addEventListener("resize", measure);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", measure);
		};
	}, [shellRef]);
	return metrics;
}
