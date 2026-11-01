import { defaultPreferences } from "@repo/ui";
import { describe, expect, it } from "vite-plus/test";
import { preferencesPatch } from "./preferences-patch";

describe("preferencesPatch", () => {
	it("sends everything from the desktop shell", () => {
		expect(preferencesPatch(defaultPreferences, false)).toEqual(
			defaultPreferences,
		);
	});

	it("leaves the layout out on a phone", () => {
		const patch = preferencesPatch(defaultPreferences, true);
		expect(patch).not.toHaveProperty("layout");
		expect(patch.theme).toEqual(defaultPreferences.theme);
		expect(patch.playback).toEqual(defaultPreferences.playback);
	});
});
