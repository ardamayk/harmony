/**
 * Palette slot contract (dark → light). Each preset CSS file defines --theme-1 … --theme-6.
 * layers.css maps slots to semantic tokens; optional semantic/*.css may override per preset.
 */
/** Which palette slot each semantic role uses (see layers.css). */
export const themeSlotContract = {
	dark: {
		mainBackground: "theme-1",
		shellSidebar: "theme-2",
		shellQueue: "theme-2",
		shellPlayer: "theme-1",
		heading: "theme-6",
		bodyText: "theme-5",
		captionBlend: ["theme-5", "theme-6"] as const,
		chrome: ["theme-3", "theme-4"] as const,
	},
	light: {
		mainBackground: "theme-6",
		shellSidebar: "theme-5",
		shellQueue: "theme-5",
		shellPlayer: "theme-6",
		heading: "theme-1",
		bodyText: "theme-2",
		captionBlend: ["theme-2", "theme-1"] as const,
		chrome: ["theme-3", "theme-4"] as const,
	},
} as const;
