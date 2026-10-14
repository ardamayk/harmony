import { describe, expect, it } from "vite-plus/test";
import { getApiBaseUrl } from "#/env";

describe("env", () => {
	it("defaults API base URL to empty string for proxy", () => {
		expect(getApiBaseUrl()).toBe("");
	});
});
