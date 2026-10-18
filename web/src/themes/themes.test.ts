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

type Rgb = [number, number, number];

const linear = (c: number) =>
	c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const gamma = (c: number) =>
	c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;

function rgbToOklab(rgb: Rgb): Rgb {
	const [r, g, b] = rgb.map(linear);
	const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
	const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
	const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
	return [
		0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
		1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
		0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
	];
}

function oklabToRgb([L, a, b]: Rgb): Rgb {
	const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
	const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
	const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
	return [
		4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
		-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
		-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
	].map((v) => Math.min(1, Math.max(0, gamma(v)))) as Rgb;
}

const toHex = (rgb: Rgb) =>
	`#${rgb
		.map((c) =>
			Math.round(c * 255)
				.toString(16)
				.padStart(2, "0"),
		)
		.join("")}` as HexColor;

function splitArgs(args: string): string[] {
	const parts: string[] = [];
	let depth = 0;
	let start = 0;
	for (let i = 0; i < args.length; i += 1) {
		if (args[i] === "(") depth += 1;
		if (args[i] === ")") depth -= 1;
		if (args[i] === "," && depth === 0) {
			parts.push(args.slice(start, i).trim());
			start = i + 1;
		}
	}
	return [...parts, args.slice(start).trim()];
}

// Collects the custom properties one preset and mode resolve to, in the
// stylesheet order of styles.css; descendant rules (shell surfaces) are skipped.
function resolvedTokens(id: string, dark: boolean) {
	const files = [
		`${id}.css`,
		"layers.css",
		"semantic/coastal-mist.css",
		"semantic/vintage-harbor.css",
		"contrast.css",
	];
	const mode = dark ? ".dark" : ":not(.dark)";
	const matching = new Set([
		`[data-theme-preset="${id}"]`,
		`[data-theme-preset]${mode}`,
		`[data-theme-preset="${id}"]${mode}`,
	]);
	const decls = new Map<string, string>();
	for (const file of files) {
		const css = readFileSync(join(themesDir, file), "utf8").replace(
			/\/\*[\s\S]*?\*\//g,
			"",
		);
		for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
			if (!selectors.split(",").some((s) => matching.has(s.trim()))) continue;
			for (const [, name, value] of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
				decls.set(name, value.replace(/\s+/g, " ").trim());
			}
		}
	}

	const evaluate = (value: string): Rgb => {
		if (value.startsWith("#")) {
			return [1, 3, 5].map(
				(i) => Number.parseInt(value.slice(i, i + 2), 16) / 255,
			) as Rgb;
		}
		if (value === "white") return [1, 1, 1];
		if (value === "black") return [0, 0, 0];
		const fn = value.match(/^([\w-]+)\((.*)\)$/);
		if (!fn) throw new Error(`Cannot resolve ${value}`);
		const [, name, args] = fn;
		if (name === "var") return token(args.trim());
		if (name === "oklch") {
			const [l, c, h] = args.split(/\s+/).map((n) => Number.parseFloat(n));
			const rad = (h * Math.PI) / 180;
			return oklabToRgb([l / 100, c * Math.cos(rad), c * Math.sin(rad)]);
		}
		if (name === "color-mix") {
			const [space, ...colors] = splitArgs(args);
			const [a, b] = colors.map((part) => {
				const pct = part.match(/\s(\d+(?:\.\d+)?)%$/);
				return {
					color: evaluate(pct ? part.slice(0, pct.index).trim() : part),
					weight: pct ? Number(pct[1]) / 100 : undefined,
				};
			});
			const wa = a.weight ?? (b.weight === undefined ? 0.5 : 1 - b.weight);
			const lerp = (x: Rgb, y: Rgb) =>
				x.map((v, i) => v * wa + y[i] * (1 - wa)) as Rgb;
			return space === "in oklab"
				? oklabToRgb(lerp(rgbToOklab(a.color), rgbToOklab(b.color)))
				: lerp(a.color, b.color);
		}
		throw new Error(`Cannot resolve ${value}`);
	};
	const token = (name: string): Rgb => {
		const value = decls.get(name);
		if (!value) throw new Error(`${id}: ${name} is not set`);
		return evaluate(value);
	};
	return (name: string) => toHex(token(name));
}

describe("text tiers", () => {
	for (const preset of THEME_PRESET_REGISTRY) {
		for (const dark of [true, false]) {
			it(`${preset.id} ${dark ? "dark" : "light"}: heading ≥ text > caption, caption and text on active ≥ 4.5:1`, () => {
				const token = resolvedTokens(preset.id, dark);
				const background = token("--background");
				const tier = (name: string) => contrastRatio(token(name), background);
				expect(tier("--heading")).toBeGreaterThanOrEqual(tier("--text"));
				expect(tier("--text")).toBeGreaterThan(tier("--caption"));

				// Vintage Harbor light keeps the Player Bar dark and re-scopes
				// its text there (contrast.css), so only page and card apply.
				const surfaces =
					preset.id === "vintage-harbor" && !dark
						? ["--background", "--card"]
						: ["--background", "--card", "--player", "--player-pill"];
				for (const surface of surfaces) {
					expect(
						contrastRatio(token("--caption"), token(surface)),
						`caption on ${surface}`,
					).toBeGreaterThanOrEqual(4.5);
				}
				// The playing Queue row sits on the active step with body text.
				if (surfaces.includes("--player")) {
					expect(
						contrastRatio(token("--text"), token("--shell-active")),
						"text on --shell-active",
					).toBeGreaterThanOrEqual(4.5);
				}
			});
		}
	}
});
