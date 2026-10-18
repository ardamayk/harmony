import type { ThemePreferences } from "@repo/api-client";
import type { HexColor } from "./palette-utils";

export type ThemePalette = readonly [
	HexColor,
	HexColor,
	HexColor,
	HexColor,
	HexColor,
	HexColor,
];

export type ThemePresetId = ThemePreferences["preset"];

export type ThemePresetMeta = {
	id: ThemePresetId;
	label: string;
	swatches: ThemePalette;
};

type ThemePresetDefinition = {
	id: ThemePresetId;
	label: string;
	/** Six hex colors, darkest (theme-1) → lightest (theme-6). Must match `{id}.css`. */
	colors: ThemePalette;
	/** When true, `semantic/{id}.css` may override semantic tokens after layers.css. */
	semanticOverride?: boolean;
};

/**
 * Single registry for theme presets. When adding a theme:
 * 1. Add an entry here (keep API OpenAPI enum in sync).
 * 2. Add `themes/{id}.css` with the same six --theme-N values.
 * 3. Import the CSS in `themes/palettes.css`.
 * 4. Add `semantic/{id}.css` only if the palette needs token overrides.
 */
export const THEME_PRESET_REGISTRY: ThemePresetDefinition[] = [
	{
		id: "earthly",
		label: "Earthly",
		colors: ["#0a1418", "#152428", "#2f6a4a", "#4fb8b2", "#a8d4c8", "#e7f0e8"],
	},
	{
		id: "tokyo-night",
		label: "Tokyo Night",
		colors: ["#1a1b26", "#24283b", "#414868", "#565f89", "#a9b1d6", "#e8eaf5"],
	},
	{
		id: "vintage-harbor",
		label: "Vintage Harbor",
		colors: ["#0a1f35", "#0a2947", "#8b5e3c", "#6b7d8f", "#d3d4c0", "#f3e4c9"],
		semanticOverride: true,
	},
	{
		id: "night-ember",
		label: "Night Ember",
		colors: ["#1a0d42", "#1e104e", "#6b4f8f", "#ff653f", "#ffc85c", "#f0e8ff"],
	},
	{
		id: "dusty-earth",
		label: "Dusty Earth",
		colors: ["#2a2420", "#3a3230", "#7a6a62", "#8a6f66", "#c4b5a8", "#f7f0e4"],
	},
	{
		id: "coastal-mist",
		label: "Coastal Mist",
		colors: ["#1a3040", "#243d4d", "#4a86a8", "#6a8fa3", "#d4e7f2", "#f7fafb"],
		semanticOverride: true,
	},
	{
		id: "sage-hearth",
		label: "Sage Hearth",
		colors: ["#241f1b", "#2e2824", "#5a6b58", "#7fa87e", "#c9b89e", "#faf4e8"],
	},
];

export const THEME_PRESET_IDS = THEME_PRESET_REGISTRY.map(
	(preset) => preset.id,
) as ThemePresetId[];
