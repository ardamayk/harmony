import { describe, expect, it } from "vite-plus/test";
import {
	GUEST_HIDDEN_FEATURES,
	isGuestBlockedRoute,
	isHiddenFromRole,
} from "./guest-access";

describe("guest access", () => {
	it("hides every listed feature from a Guest and only from a Guest", () => {
		for (const feature of GUEST_HIDDEN_FEATURES) {
			expect(isHiddenFromRole("guest", feature)).toBe(true);
			expect(isHiddenFromRole("owner", feature)).toBe(false);
			expect(isHiddenFromRole("none", feature)).toBe(false);
			expect(isHiddenFromRole("loading", feature)).toBe(false);
			expect(isHiddenFromRole(undefined, feature)).toBe(false);
		}
	});

	it("names each control the Guest interface must not offer", () => {
		expect([...GUEST_HIDDEN_FEATURES]).toEqual([
			"album-deletion",
			"album-radio",
			"external-source",
			"favorites",
			"listen-together-page",
			"managed-import",
			"playlists",
			"queue-editing",
			"radio",
			"server-settings",
			"track-deletion",
			"track-replacement",
		]);
	});

	it("blocks whole route trees without matching look-alike paths", () => {
		expect(isGuestBlockedRoute("/playlists")).toBe(true);
		expect(isGuestBlockedRoute("/playlists/1")).toBe(true);
		expect(isGuestBlockedRoute("/radio/discover")).toBe(true);
		expect(isGuestBlockedRoute("/favorites")).toBe(true);
		expect(isGuestBlockedRoute("/listen-together")).toBe(true);
		expect(isGuestBlockedRoute("/radiohead")).toBe(false);
		expect(isGuestBlockedRoute("/library/albums")).toBe(false);
		expect(isGuestBlockedRoute("/settings")).toBe(false);
	});
});
