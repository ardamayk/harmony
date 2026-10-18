import type { ThemePreferences } from "@repo/api-client";
import { useEffect } from "react";
import { useResolvedThemeMode } from "./use-resolved-theme-mode";

export function ThemeProvider({ theme }: { theme: ThemePreferences }) {
	const resolvedMode = useResolvedThemeMode(theme.mode);
	useEffect(() => {
		const root = document.documentElement;
		root.dataset.themePreset = theme.preset;
		root.classList.toggle("dark", resolvedMode === "dark");
	}, [resolvedMode, theme.preset]);
	return null;
}
