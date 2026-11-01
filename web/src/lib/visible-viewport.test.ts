import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { fitToVisibleViewport } from "./visible-viewport";

describe("fitToVisibleViewport", () => {
	afterEach(() => vi.unstubAllGlobals());

	it("follows the visual viewport until cleaned up", () => {
		const viewport = Object.assign(new EventTarget(), {
			height: 874,
			offsetTop: 0,
		});
		vi.stubGlobal("visualViewport", viewport);
		const element = document.createElement("div");

		const stop = fitToVisibleViewport(element);
		expect(element.style.height).toBe("874px");

		viewport.height = 538;
		viewport.offsetTop = 12;
		viewport.dispatchEvent(new Event("resize"));
		expect(element.style.height).toBe("538px");
		expect(element.style.top).toBe("12px");

		stop();
		viewport.height = 874;
		viewport.dispatchEvent(new Event("resize"));
		expect(element.style.height).toBe("538px");
	});
});
