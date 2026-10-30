import type { UserPreferences } from "@repo/api-client";
import { defaultPreferences } from "@repo/ui";

const GUEST_PREFERENCES_KEY = "listen-together-guest-preferences";

/** A Guest's Preferences stay on their device (ADR 0037). */
export function readGuestPreferences(): UserPreferences {
	try {
		const stored = localStorage.getItem(GUEST_PREFERENCES_KEY);
		return stored
			? { ...defaultPreferences, ...JSON.parse(stored) }
			: defaultPreferences;
	} catch {
		return defaultPreferences;
	}
}

export function writeGuestPreferences(preferences: UserPreferences): void {
	try {
		localStorage.setItem(GUEST_PREFERENCES_KEY, JSON.stringify(preferences));
	} catch (error) {
		console.warn("Failed to save Guest preferences", { error });
	}
}
