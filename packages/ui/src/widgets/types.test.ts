import { describe, expect, it } from "vite-plus/test";
import {
	clampPanelSizes,
	deriveShellLayout,
	normalizeLayout,
} from "../widgets/layout-utils";
import { defaultLayout } from "../widgets/types";

describe("layout utils", () => {
	it("fills missing sizes with defaults", () => {
		const result = normalizeLayout({
			sidebarPosition: "left",
			panels: { left: ["now-playing"], right: ["discover"] },
			collapsed: { left: false, right: false },
		});
		expect(result.sizes).toEqual(defaultLayout.sizes);
	});

	it("clamps collapsed drag sizes back to readable minimums", () => {
		const result = clampPanelSizes([3, 94, 3]);
		expect(result[0]).toBeGreaterThanOrEqual(15);
		expect(result[2]).toBeGreaterThanOrEqual(18);
		expect(result[0] + result[1] + result[2]).toBeCloseTo(100, 0);
	});

	it("allows mini widths when a panel is collapsed", () => {
		const result = clampPanelSizes([5, 70, 25], { left: true, right: false });
		expect(result[0]).toBe(5);
		expect(result[0] + result[1] + result[2]).toBeCloseTo(100, 0);
	});

	it("derives resizable layout around the fixed nav panel", () => {
		const result = deriveShellLayout(defaultLayout);

		expect(result.navPanel).toBe("left");
		expect(result.queuePanel).toBe("right");
		expect(result.leftVisible).toBe(false);
		expect(result.rightVisible).toBe(true);
		expect(result.visibleResizableLayout).toEqual({ main: 64.1, right: 35.9 });
		expect(result.toPanelSizes({ main: 64.1, right: 35.9 })).toEqual([
			22, 50, 28,
		]);
	});

	it("folds a collapsed queue panel into the main resizable panel", () => {
		const result = deriveShellLayout({
			...defaultLayout,
			collapsed: { left: false, right: true },
			sizes: [22, 73, 5],
		});

		expect(result.rightVisible).toBe(false);
		expect(result.visibleResizableLayout).toEqual({ main: 100 });
		expect(result.toPanelSizes({ main: 100 })).toEqual([22, 73, 5]);
	});
});
