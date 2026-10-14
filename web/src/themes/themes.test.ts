import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vite-plus/test";
import {
	contrastRatio,
	type HexColor,
	isDarkToLightOrder,
	parsePaletteFromCss,
} from "./palette-utils";
import { THEME_PRESET_IDS, THEME_PRESET_REGISTRY } from "./registry";
import { themeSlotContract } from "./tokens";

const themesDir = join(import.meta.dirname);
const layersCss = readFileSync(join(themesDir, "layers.css"), "utf8");
const openapiYaml = readFileSync(
	join(import.meta.dirname, "../../../packages/contracts/openapi.yaml"),
	"utf8",
);

function paletteCssPath(id: string): string {
	return join(themesDir, `${id}.css`);
}

function semanticCssPath(id: string): string {
	return join(themesDir, "semantic", `${id}.css`);
}

function readPaletteCss(id: string): string {
	return readFileSync(paletteCssPath(id), "utf8");
}

function slotColor(
	colors: readonly HexColor[],
	slot: `theme-${1 | 2 | 3 | 4 | 5 | 6}`,
): HexColor {
	return colors[Number(slot.replace("theme-", "")) - 1];
}

describe("theme registry", () => {
	it("matches OpenAPI preset enum", () => {
		const enumBlock = openapiYaml.match(
			/preset:\s*\n\s*type: string\s*\n\s*enum:\s*([\s\S]*?)(?:\n\s{4}\w|\n {4}[A-Z])/m,
		);
		if (!enumBlock) {
			throw new Error("Theme preset enum not found in OpenAPI schema");
		}
		const openapiIds = [...enumBlock[1].matchAll(/- ([a-z0-9-]+)/g)].map(
			(match) => match[1],
		);
		expect(THEME_PRESET_IDS).toEqual(openapiIds);
	});

	it("has a palette CSS file per preset", () => {
		for (const preset of THEME_PRESET_REGISTRY) {
			expect(() => readPaletteCss(preset.id)).not.toThrow();
		}
	});

	it("keeps registry colors in sync with palette CSS files", () => {
		for (const preset of THEME_PRESET_REGISTRY) {
			const parsed = parsePaletteFromCss(readPaletteCss(preset.id));
			expect(parsed).toEqual(preset.colors.map((c) => c.toLowerCase()));
		}
	});

	it("orders palette colors dark → light", () => {
		for (const preset of THEME_PRESET_REGISTRY) {
			expect(isDarkToLightOrder(preset.colors)).toBe(true);
		}
	});

	it("declares semantic overrides only when the file exists", () => {
		for (const preset of THEME_PRESET_REGISTRY) {
			if (preset.semanticOverride) {
				expect(() =>
					readFileSync(semanticCssPath(preset.id), "utf8"),
				).not.toThrow();
			}
		}
	});

	it("declares Vintage Harbor shell overrides for Figma shell bars", () => {
		const vintageHarbor = THEME_PRESET_REGISTRY.find(
			(preset) => preset.id === "vintage-harbor",
		);
		const css = readFileSync(semanticCssPath("vintage-harbor"), "utf8");

		expect(vintageHarbor?.semanticOverride).toBe(true);
		expect(css).toContain('[data-theme-preset="vintage-harbor"]');
		expect(css).toContain("--sidebar: #132030");
		expect(css).toContain("--shell-brand: #e6bf9e");
		expect(css).toContain("--shell-active: rgba(40, 54, 70, 0.5)");
		expect(css).toContain("--player: rgba(30, 43, 59, 0.95)");
	});
});

describe("theme slot contract", () => {
	it("layers.css maps readable text to palette poles, not chrome slots", () => {
		expect(layersCss).toContain("--heading: var(--theme-6)");
		expect(layersCss).toContain("--text: var(--theme-5)");
		expect(layersCss).not.toMatch(/--foreground:\s*var\(--theme-[34]\)/);
		expect(layersCss).not.toMatch(
			/--sidebar-foreground:\s*var\(--theme-[34]\)/,
		);
	});

	it("layers.css maps shell surfaces per dark/light contract", () => {
		expect(layersCss).toContain("--sidebar: var(--theme-2)");
		expect(layersCss).toContain("--queue: var(--theme-2)");
		// The floating player floats over --background, so it sits one step
		// above cards instead of sharing the background slot.
		expect(layersCss).toContain(
			"--player: color-mix(in srgb, var(--theme-2) 85%, var(--theme-3) 15%)",
		);
		expect(layersCss).toContain("--sidebar: var(--theme-5)");
		expect(layersCss).toContain("--queue: var(--theme-5)");
		expect(layersCss).toContain(
			"--player: color-mix(in srgb, var(--theme-5) 85%, var(--theme-4) 15%)",
		);
	});

	it("layers.css gives the floating player its own border and shadow tokens", () => {
		expect(layersCss.match(/--player-border:/g)).toHaveLength(2);
		expect(layersCss.match(/--player-shadow:/g)).toHaveLength(2);
	});

	it("layers.css exposes shell component semantic tokens", () => {
		expect(layersCss).toContain("--shell-brand: var(--sidebar-heading)");
		expect(layersCss).toContain("--shell-active: var(--sidebar-accent)");
		expect(layersCss).toContain("--player-control-primary: var(--primary)");
		expect(layersCss).toContain("--player-live-progress: var(--primary)");
	});

	it("meets minimum contrast for default dark/light text on shell surfaces", () => {
		for (const preset of THEME_PRESET_REGISTRY) {
			const colors = preset.colors;
			const dark = themeSlotContract.dark;
			const light = themeSlotContract.light;

			expect(
				contrastRatio(
					slotColor(colors, dark.heading),
					slotColor(colors, dark.shellSidebar),
				),
			).toBeGreaterThanOrEqual(4.5);
			expect(
				contrastRatio(
					slotColor(colors, dark.bodyText),
					slotColor(colors, dark.shellPlayer),
				),
			).toBeGreaterThanOrEqual(4.5);
			expect(
				contrastRatio(
					slotColor(colors, light.heading),
					slotColor(colors, light.shellSidebar),
				),
			).toBeGreaterThanOrEqual(4.5);
			expect(
				contrastRatio(
					slotColor(colors, light.bodyText),
					slotColor(colors, light.mainBackground),
				),
			).toBeGreaterThanOrEqual(4.5);
		}
	});
});

describe("themePresetOptions", () => {
	it("exposes six swatches per preset", async () => {
		const { themePresetOptions } = await import("./presets");
		for (const option of themePresetOptions) {
			expect(option.swatches).toHaveLength(6);
		}
	});
});
