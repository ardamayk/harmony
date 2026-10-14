import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";

describe("settings route", () => {
	it("persists preferences only through LayoutProvider onPreferencesChange", () => {
		const source = readFileSync(
			"src/routes/settings/-settings-page.tsx",
			"utf8",
		);

		expect(source).not.toContain("patchPreferences.mutate");
	});
});
