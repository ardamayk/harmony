import type { ThemePresetMeta } from "./registry";
import { THEME_PRESET_REGISTRY } from "./registry";

export const themePresetOptions: ThemePresetMeta[] = THEME_PRESET_REGISTRY.map(
	({ id, label, hues }) => {
		const neutral = hues.neutralChroma ?? 1;
		const accent = hues.accentChroma ?? 1;
		return {
			id,
			label,
			// The dark ladder's page, support and accent steps (dark-ladder.css).
			swatches: [
				`oklch(22% ${0.022 * neutral} ${hues.neutral})`,
				`oklch(58.2% ${0.05 * neutral} ${hues.neutral})`,
				`oklch(83% ${0.063 * accent} ${hues.accent})`,
			],
		};
	},
);
