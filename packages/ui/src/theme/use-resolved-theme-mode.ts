import type { ThemePreferences } from "@repo/api-client";
import { useEffect, useState } from "react";

type ResolvedThemeMode = "light" | "dark";

function resolveThemeMode(mode: ThemePreferences["mode"]): ResolvedThemeMode {
	if (mode === "dark") return "dark";
	if (mode === "light") return "light";
	if (typeof window.matchMedia !== "function") return "light";
	return window.matchMedia("(prefers-color-scheme: dark)").matches
		? "dark"
		: "light";
}

/**
 * Resolves a Theme Preference ("system" included) to the mode currently in
 * effect and follows the OS preference while "system" is selected.
 */
export function useResolvedThemeMode(
	mode: ThemePreferences["mode"],
): ResolvedThemeMode {
	const [resolved, setResolved] = useState<ResolvedThemeMode>(() =>
		resolveThemeMode(mode),
	);
	useEffect(() => {
		setResolved(resolveThemeMode(mode));
		if (mode !== "system" || typeof window.matchMedia !== "function") return;
		const media = window.matchMedia("(prefers-color-scheme: dark)");
		const onChange = () => setResolved(resolveThemeMode(mode));
		media.addEventListener("change", onChange);
		return () => media.removeEventListener("change", onChange);
	}, [mode]);
	return resolved;
}
