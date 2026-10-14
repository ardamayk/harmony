import type { LayoutPreferences } from "@repo/api-client";
import { defaultLayout } from "./types";

const MIN_LEFT = 15;
const MAX_LEFT = 45;
const MIN_RIGHT = 18;
const MAX_RIGHT = 50;
const MIN_MAIN = 25;
const MIN_MINI = 4;

export const MINI_PANEL_SIZE = 5;
const MINI_PANEL_MAX = 12;

type PanelSide = "left" | "right";

type PanelLayout = {
	left: number;
	main: number;
	right: number;
};

type VisibleResizableLayout = Partial<Record<PanelSide, number>> & {
	main: number;
};

type ShellLayout = {
	sizes: [number, number, number];
	panelLayout: PanelLayout;
	navPanel: PanelSide;
	queuePanel: PanelSide;
	navCollapsed: boolean;
	queueCollapsed: boolean;
	leftVisible: boolean;
	rightVisible: boolean;
	leftMin: `${number}`;
	leftMax: `${number}`;
	rightMin: `${number}`;
	rightMax: `${number}`;
	visibleResizableLayout: VisibleResizableLayout;
	toPanelSizes: (next: Record<string, number>) => [number, number, number];
};

function getNavPanel(
	sidebarPosition: LayoutPreferences["sidebarPosition"],
): PanelSide {
	return sidebarPosition === "left" ? "left" : "right";
}

export function getQueuePanel(
	sidebarPosition: LayoutPreferences["sidebarPosition"],
): PanelSide {
	return sidebarPosition === "left" ? "right" : "left";
}

function pct(n: number): `${number}` {
	return String(n) as `${number}`;
}

function roundPanelSize(size: number): number {
	return Math.round(size * 10) / 10;
}

/** Enforce readable minimums; allow wide side panels up to generous maxima. */
export function clampPanelSizes(
	sizes: number[],
	collapsed: { left?: boolean; right?: boolean } = {},
): [number, number, number] {
	const fallback = defaultLayout.sizes ?? [22, 50, 28];
	let left = sizes[0] ?? fallback[0];
	let right = sizes[2] ?? fallback[2];

	if (collapsed.left) {
		left = Math.max(4, Math.min(MINI_PANEL_MAX, left));
	} else {
		left = Math.max(MIN_LEFT, Math.min(MAX_LEFT, left));
	}

	if (collapsed.right) {
		right = Math.max(4, Math.min(MINI_PANEL_MAX, right));
	} else {
		right = Math.max(MIN_RIGHT, Math.min(MAX_RIGHT, right));
	}

	let main = 100 - left - right;
	if (main < MIN_MAIN) {
		const deficit = MIN_MAIN - main;
		const sideSum = left + right;
		left -= (left / sideSum) * deficit;
		right -= (right / sideSum) * deficit;
		main = MIN_MAIN;
	}

	return [roundPanelSize(left), roundPanelSize(main), roundPanelSize(right)];
}

export function normalizeLayout(layout: LayoutPreferences): LayoutPreferences {
	const base = defaultLayout;
	const rawSizes =
		layout.sizes?.length === 3 ? layout.sizes : (base.sizes ?? [22, 50, 28]);
	const collapsed = {
		left: layout.collapsed?.left ?? base.collapsed.left,
		right: layout.collapsed?.right ?? base.collapsed.right,
	};

	return {
		sidebarPosition: layout.sidebarPosition ?? base.sidebarPosition,
		panels: {
			left: layout.panels?.left ?? base.panels.left,
			right: layout.panels?.right ?? base.panels.right,
		},
		collapsed,
		sizes: clampPanelSizes(rawSizes, collapsed),
	};
}

export function deriveShellLayout(layout: LayoutPreferences): ShellLayout {
	const sizes = clampPanelSizes(layout.sizes ?? [22, 50, 28], layout.collapsed);
	const panelLayout = { left: sizes[0], main: sizes[1], right: sizes[2] };
	const navPanel = getNavPanel(layout.sidebarPosition);
	const queuePanel = getQueuePanel(layout.sidebarPosition);
	const navCollapsed = layout.collapsed[navPanel];
	const queueCollapsed = layout.collapsed[queuePanel];
	const fixedNavSize = panelLayout[navPanel];
	const leftVisible =
		!(queueCollapsed && queuePanel === "left") && navPanel !== "left";
	const rightVisible =
		!(queueCollapsed && queuePanel === "right") && navPanel !== "right";
	const resizableTotal = 100 - fixedNavSize;
	const toResizablePct = (size: number): number =>
		resizableTotal > 0 ? roundPanelSize((size / resizableTotal) * 100) : size;
	const fromResizablePct = (
		size: number | undefined,
		fallback: number,
	): number =>
		size === undefined
			? fallback
			: roundPanelSize((size / 100) * resizableTotal);

	return {
		sizes,
		panelLayout,
		navPanel,
		queuePanel,
		navCollapsed,
		queueCollapsed,
		leftVisible,
		rightVisible,
		leftMin: pct(
			layout.collapsed.left && queuePanel !== "left" ? MIN_MINI : MIN_LEFT,
		),
		leftMax: pct(
			layout.collapsed.left && queuePanel !== "left"
				? MINI_PANEL_MAX
				: MAX_LEFT,
		),
		rightMin: pct(
			layout.collapsed.right && queuePanel !== "right" ? MIN_MINI : MIN_RIGHT,
		),
		rightMax: pct(
			layout.collapsed.right && queuePanel !== "right"
				? MINI_PANEL_MAX
				: MAX_RIGHT,
		),
		visibleResizableLayout: {
			...(leftVisible ? { left: toResizablePct(panelLayout.left) } : {}),
			main:
				toResizablePct(panelLayout.main) +
				(!leftVisible && queuePanel === "left"
					? toResizablePct(panelLayout.left)
					: 0) +
				(!rightVisible && queuePanel === "right"
					? toResizablePct(panelLayout.right)
					: 0),
			...(rightVisible ? { right: toResizablePct(panelLayout.right) } : {}),
		},
		toPanelSizes: (next) =>
			clampPanelSizes(
				[
					navPanel === "left"
						? fixedNavSize
						: fromResizablePct(next.left, sizes[0]),
					fromResizablePct(next.main, sizes[1]),
					navPanel === "right"
						? fixedNavSize
						: fromResizablePct(next.right, sizes[2]),
				],
				layout.collapsed,
			),
	};
}
