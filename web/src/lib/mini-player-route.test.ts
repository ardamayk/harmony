import { describe, expect, it } from "vite-plus/test";
import { isMiniPlayerRoute } from "./mini-player-route";

describe("isMiniPlayerRoute", () => {
	it("matches only the mini player path", () => {
		expect(isMiniPlayerRoute("/mini")).toBe(true);
		expect(isMiniPlayerRoute("/mini/")).toBe(true);
		expect(isMiniPlayerRoute("/")).toBe(false);
		expect(isMiniPlayerRoute("/minimal")).toBe(false);
		expect(isMiniPlayerRoute("/library/mini")).toBe(false);
	});
});
