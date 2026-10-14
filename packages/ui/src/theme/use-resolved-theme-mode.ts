import type { ThemePreferences } from "@repo/api-client";
import { useSyncExternalStore } from "react";

type ResolvedThemeMode = "light" | "dark";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function subscribeToSystemTheme(onChange: () => void): () => void {
	if (typeof window.matchMedia !== "function") return () => {};
	const media = window.matchMedia(DARK_QUERY);
	media.addEventListener("change", onChange);
	return () => media.removeEventListener("change", onChange);
}

function systemPrefersDark(): boolean {
	return (
		typeof window.matchMedia === "function" &&
		window.matchMedia(DARK_QUERY).matches
	);
}

/**
 * Resolves a Theme Preference ("system" included) to the mode currently in
 * effect and follows the OS preference while "system" is selected.
 */
export function useResolvedThemeMode(
	mode: ThemePreferences["mode"],
): ResolvedThemeMode {
	const prefersDark = useSyncExternalStore(
		subscribeToSystemTheme,
		systemPrefersDark,
		() => false,
	);
	if (mode === "dark" || mode === "light") return mode;
	return prefersDark ? "dark" : "light";
}
