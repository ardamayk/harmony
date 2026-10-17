import { describe, expect, it } from "vite-plus/test";
import {
	calculateLibrarySearchBounds,
	calculateQueueDrawerWidth,
} from "./use-queue-drawer-width";

it("places Search between third card centers of the full eight-column grid", () => {
	// Eight cards occupy 2084px; the 118px residual stays on the right.
	expect(calculateLibrarySearchBounds(2250, 24, 16)).toEqual({
		width: 786,
		center: 1066,
	});
	expect(calculateLibrarySearchBounds(375, 24, 16)).toEqual({
		width: 343,
		center: 187.5,
	});
});

describe("calculateQueueDrawerWidth", () => {
	it("leaves six fixed cards and a 24px margin in the original wide-shell layout", () => {
		const panelWidth = calculateQueueDrawerWidth(2150, 24, 16);
		expect(panelWidth).toBe(518);
		const availableWidth = 2102;
		const collectionWidth = availableWidth - panelWidth - 24;
		expect(collectionWidth).toBe(1560);
		// Six 250px cards with five 12px gaps fit; a seventh starts the next row.
		expect(collectionWidth).toBe(6 * 250 + 5 * 12);
		expect(collectionWidth).toBeLessThan(7 * 250 + 6 * 12);
		const lastCardRight = 24 + collectionWidth;
		const panelLeft = 2150 - 24 - panelWidth;
		expect(panelLeft - lastCardRight).toBe(24);
	});

	it("gives additional wide-screen space to the drawer instead of a seventh column", () => {
		const panelWidth = calculateQueueDrawerWidth(2600, 24, 16);
		expect(panelWidth).toBe(968);
		expect(2600 - 48 - panelWidth - 24).toBe(1560);
	});

	it.each([
		{ shellWidth: 1920, columns: 6, panelWidth: 288 },
		{ shellWidth: 1919, columns: 5, panelWidth: 549 },
		{ shellWidth: 1658, columns: 5, panelWidth: 288 },
		{ shellWidth: 1657, columns: 4, panelWidth: 549 },
		{ shellWidth: 1396, columns: 4, panelWidth: 288 },
		{ shellWidth: 1395, columns: 3, panelWidth: 549 },
		{ shellWidth: 1134, columns: 3, panelWidth: 288 },
		{ shellWidth: 1133, columns: 2, panelWidth: 549 },
		{ shellWidth: 1024, columns: 2, panelWidth: 440 },
	])(
		"fits $columns columns without overlap at shell width $shellWidth",
		({ shellWidth, columns, panelWidth }) => {
			const width = calculateQueueDrawerWidth(shellWidth, 24, 16);
			expect(width).toBe(panelWidth);
			expect(width).toBeGreaterThanOrEqual(288);
			const cardsRight = 24 + columns * 250 + (columns - 1) * 12;
			const drawerLeft = shellWidth - 24 - width;
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

	it("respects rem-based card gaps, minimum width and margins with a larger root font", () => {
		const panelWidth = calculateQueueDrawerWidth(2150, 24, 20);
		expect(panelWidth).toBe(497);
		const collectionWidth = 6 * 250 + 5 * 15;
		expect(collectionWidth).toBe(1575);
		expect(2102 - collectionWidth - panelWidth).toBe(30);
		expect(calculateQueueDrawerWidth(1000, 24, 20)).toBe(386);
	});
});
