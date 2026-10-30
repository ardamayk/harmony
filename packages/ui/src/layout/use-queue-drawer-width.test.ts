import { describe, expect, it } from "vite-plus/test";
import { calculateQueueDrawerWidth } from "./use-queue-drawer-width";

describe("calculateQueueDrawerWidth", () => {
	it.each([1024, 1280, 1920, 2560])(
		"keeps one width from lg up at %i",
		(shellWidth) => {
			expect(calculateQueueDrawerWidth(shellWidth, 24, 200)).toBe(400);
		},
	);

	it.each([
		{ shellWidth: 1023, expected: 350 },
		{ shellWidth: 768, expected: 350 },
	])(
		"uses the fixed below-lg width at $shellWidth",
		({ shellWidth, expected }) => {
			expect(calculateQueueDrawerWidth(shellWidth, 24)).toBe(expected);
		},
	);

	it("never takes more than the Sidebar and the insets leave", () => {
		expect(calculateQueueDrawerWidth(1024, 24, 800)).toBe(176);
	});
});
