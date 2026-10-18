import { describe, expect, it } from "vite-plus/test";
import { calculateQueueDrawerWidth } from "./use-queue-drawer-width";

describe("calculateQueueDrawerWidth", () => {
	it("leaves six fixed cards and one page inset before the docked drawer", () => {
		const panelWidth = calculateQueueDrawerWidth(2150, 24, 16);
		expect(panelWidth).toBe(542);
		const collectionWidth = 2102 - panelWidth;
		// Six 250px cards with five 12px gaps fit; a seventh starts the next row.
		expect(collectionWidth).toBe(6 * 250 + 5 * 12);
		expect(collectionWidth).toBeLessThan(7 * 250 + 6 * 12);
		const lastCardRight = 24 + collectionWidth;
		// Docked: the drawer's right edge is the window edge.
		const panelLeft = 2150 - panelWidth;
		expect(panelLeft - lastCardRight).toBe(24);
	});

	it("leaves the Sidebar's width out of what cards and drawer share", () => {
		// 2150px window, 240px Sidebar: six cards still fit, the drawer shrinks.
		const panelWidth = calculateQueueDrawerWidth(2150, 24, 16, 240);
		expect(panelWidth).toBe(542 - 240);
		expect(2150 - 240 - 48 - panelWidth).toBe(6 * 250 + 5 * 12);
	});

	it("gives additional wide-screen space to the drawer instead of a seventh column", () => {
		const panelWidth = calculateQueueDrawerWidth(2600, 24, 16);
		expect(panelWidth).toBe(992);
		expect(2600 - 48 - panelWidth).toBe(1560);
	});

	it.each([
		{ shellWidth: 1896, columns: 6, panelWidth: 288 },
		{ shellWidth: 1895, columns: 5, panelWidth: 549 },
		{ shellWidth: 1634, columns: 5, panelWidth: 288 },
		{ shellWidth: 1633, columns: 4, panelWidth: 549 },
		{ shellWidth: 1372, columns: 4, panelWidth: 288 },
		{ shellWidth: 1371, columns: 3, panelWidth: 549 },
		{ shellWidth: 1110, columns: 3, panelWidth: 288 },
		{ shellWidth: 1109, columns: 2, panelWidth: 549 },
		{ shellWidth: 1024, columns: 2, panelWidth: 464 },
	])(
		"fits $columns columns without overlap at shell width $shellWidth",
		({ shellWidth, columns, panelWidth }) => {
			const width = calculateQueueDrawerWidth(shellWidth, 24, 16);
			expect(width).toBe(panelWidth);
			expect(width).toBeGreaterThanOrEqual(288);
			const cardsRight = 24 + columns * 250 + (columns - 1) * 12;
			const drawerLeft = shellWidth - width;
			expect(drawerLeft - cardsRight).toBe(24);
		},
	);

	it.each([
		{ shellWidth: 1023, expected: 386 },
		{ shellWidth: 768, expected: 386 },
		{ shellWidth: 767, expected: 275 },
		{ shellWidth: 640, expected: 275 },
		{ shellWidth: 320, expected: 272 },
		{ shellWidth: 48, expected: 0 },
		{ shellWidth: 40, expected: 0 },
	])(
		"uses the fixed below-lg and below-md widths, within the space, at $shellWidth",
		({ shellWidth, expected }) => {
			expect(calculateQueueDrawerWidth(shellWidth, 24, 16)).toBe(expected);
		},
	);

	it("respects rem-based card gaps and minimum width with a larger root font", () => {
		const panelWidth = calculateQueueDrawerWidth(2150, 24, 20);
		expect(panelWidth).toBe(527);
		const collectionWidth = 6 * 250 + 5 * 15;
		expect(collectionWidth).toBe(1575);
		expect(2102 - collectionWidth - panelWidth).toBe(0);
		expect(calculateQueueDrawerWidth(1000, 24, 20)).toBe(386);
	});
});
