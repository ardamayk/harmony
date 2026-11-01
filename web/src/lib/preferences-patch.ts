import type { UserPreferences, UserPreferencesPatch } from "@repo/api-client";

/**
 * Layout belongs to the desktop shell, and the server would take a phone's
 * copy, possibly days old, over the desktop's (ADR 0041).
 */
export function preferencesPatch(
	preferences: UserPreferences,
	isPhone: boolean,
): UserPreferencesPatch {
	if (!isPhone) return preferences;
	const { theme, playback } = preferences;
	return { theme, playback };
}
