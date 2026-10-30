import { type RefObject, useLayoutEffect, useState } from "react";

// From Tailwind's `lg` (1024px) the drawer has one width and the collection
// cards stretch to the rest (ADR 0036). Below `lg` it is narrower and the
// collection grid stretches two columns into the rest, mirrored by the
// `max-lg` grid class in the Web Client's collection-grid-layout. The shell is
// never narrower than 768px.
const LG_BREAKPOINT_PX = 1024;
const QUEUE_WIDTH_PX = 400;
const QUEUE_BELOW_LG_WIDTH_PX = 350;

/**
 * `shellWidth` decides the breakpoints (it is the window's width); the drawer
 * never takes more than the Sidebar and the page's two insets leave.
 */
export function calculateQueueDrawerWidth(
	shellWidth: number,
	inset: number,
	sidebarWidth = 0,
): number {
	const availableWidth = Math.max(0, shellWidth - sidebarWidth - 2 * inset);
	if (shellWidth < LG_BREAKPOINT_PX)
		return Math.min(QUEUE_BELOW_LG_WIDTH_PX, availableWidth);
	return Math.min(QUEUE_WIDTH_PX, availableWidth);
}

/**
 * Measure the Queue Drawer from the shell, independently of Queue
 * visibility, and where the search card drops down: over the top bar's
 * search field, as wide as it.
 */
export function useQueueDrawerWidth(
	shellRef: RefObject<HTMLDivElement | null>,
) {
	const [measured, setMeasured] = useState<{
		shellWidth: number;
		inset: number;
		sidebarWidth: number;
		searchWidth: number;
		searchCenter: number;
		searchTop: number;
		/** From the field's top to the top bar's bottom edge, its border included. */
		searchRowHeight?: number;
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
			const fieldElement = shell.querySelector("[data-library-search-field]");
			const field = fieldElement?.getBoundingClientRect();
			const header = fieldElement?.closest("header")?.getBoundingClientRect();
			setMeasured({
				shellWidth,
				inset,
				sidebarWidth,
				searchWidth: field?.width ?? 0,
				searchCenter: field ? field.left + field.width / 2 : 0,
				searchTop: field?.top ?? 0,
				searchRowHeight:
					field && header ? header.bottom - field.top : undefined,
			});
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(shell);
		// The field moves and resizes without the shell resizing (the Home
		// button or brand beside it changing size, a late web font, the Top
		// Nav's drawer-wide margin once the drawer is measured), and the
		// search card must still drop exactly over it. A move without a
		// resize shows as a sibling resizing, so the bar's children are
		// watched too.
		const field = shell.querySelector("[data-library-search-field]");
		if (field) observer.observe(field);
		for (const sibling of field?.closest("header")?.children ?? []) {
			observer.observe(sibling);
		}
		window.addEventListener("resize", measure);
		return () => {
			observer.disconnect();
			window.removeEventListener("resize", measure);
		};
	}, [shellRef]);
	if (!measured) return undefined;
	// Derived on render, so a hot update of the rule applies without a resize.
	return {
		...measured,
		queueWidth: calculateQueueDrawerWidth(
			measured.shellWidth,
			measured.inset,
			measured.sidebarWidth,
		),
	};
}
