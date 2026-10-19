import type { ThemePresetMeta } from "./registry";
import { THEME_PRESET_REGISTRY } from "./registry";

export const themePresetOptions: ThemePresetMeta[] = THEME_PRESET_REGISTRY.map(
	(preset) => ({
		id: preset.id,
		label: preset.label,
		// The dark ladder's page, support and accent steps (dark-ladder.css).
		swatches: [
			`oklch(22% 0.022 ${preset.hues.neutral})`,
			`oklch(58.2% 0.05 ${preset.hues.neutral})`,
			`oklch(83% 0.063 ${preset.hues.accent})`,
		],
	}),
);
