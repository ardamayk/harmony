import { describe, expect, it } from "vite-plus/test";
import {
	accentContrast,
	accentCssVariables,
	accentFromPixels,
	clampAccent,
	rgbToHsl,
} from "./cover-accent";

function pixels(colors: Array<[number, number, number]>): Uint8ClampedArray {
	const data = new Uint8ClampedArray(colors.length * 4);
	colors.forEach(([r, g, b], index) => {
		data.set([r, g, b, 255], index * 4);
	});
	return data;
}

describe("cover accent", () => {
	it("converts RGB to HSL", () => {
		expect(rgbToHsl(255, 0, 0)).toMatchObject({ h: 0, s: 1, l: 0.5 });
		expect(rgbToHsl(128, 128, 128).s).toBe(0);
	});

	it("picks the dominant saturated hue and ignores grey", () => {
		const accent = accentFromPixels(
			pixels([
				[200, 30, 30],
				[210, 40, 40],
				[190, 20, 20],
				[30, 30, 200],
				[128, 128, 128],
				[128, 128, 128],
				[128, 128, 128],
			]),
		);
		expect(accent).not.toBeNull();
		expect(Math.round(accent?.h ?? -1)).toBe(0);
	});

	it("returns null for a monochrome image", () => {
		expect(
			accentFromPixels(
				pixels([
					[10, 10, 10],
					[240, 240, 240],
				]),
			),
		).toBeNull();
	});

	it("takes a pale colour when the soft pixels agree on one hue", () => {
		const accent = accentFromPixels(
			pixels([
				[142, 133, 146],
				[150, 140, 155],
				[30, 30, 30],
			]),
		);
		expect(accent?.muted).toBeUndefined();
		expect(accent?.soft).toBe(true);
		expect(accent?.s).toBeLessThan(0.2);
		expect(Math.round(accent?.h ?? -1)).toBeGreaterThan(270);
		expect(Math.round(accent?.h ?? -1)).toBeLessThan(295);
		expect(
			accentCssVariables(accent, "dark")["--player-control-primary"],
		).toMatch(/^hsl\(2[789]\d 1\d% /);
	});

	it("gives neutral controls when soft hues scatter", () => {
		const accent = accentFromPixels(
			pixels([
				[110, 100, 100],
				[100, 110, 100],
				[100, 100, 110],
				[90, 90, 90],
			]),
		);
		expect(accent?.muted).toBe(true);
		const dark = accentCssVariables(accent, "dark");
		expect(dark["--player-control-primary"]).toBe("hsl(0 0% 70%)");
		expect(dark["--player-control-primary-foreground"]).toBe("hsl(0 0% 8%)");
		expect(dark["--cover-tint"]).toMatch(/^hsl\(\d+ \d+% 55%\)$/);
		const light = accentCssVariables(accent, "light");
		expect(light["--player-control-primary"]).toBe("hsl(0 0% 30%)");
		expect(light["--player-control-primary-foreground"]).toBe("hsl(0 0% 98%)");
	});

	it("lifts a dark accent only as far as the bar needs for 4.5:1", () => {
		// Nightwish "Decades": a dark teal the old 55% floor washed out.
		const teal = clampAccent({ h: 165, s: 0.36, l: 0.23 }, "dark", 0.021);
		expect(accentContrast(teal, 0.021)).toBeGreaterThanOrEqual(4.5);
		expect(teal.l).toBeLessThan(0.55);
		expect(teal.h).toBe(165);
		// A light cover keeps its own lightness, up to the ceiling.
		expect(clampAccent({ h: 40, s: 0.5, l: 0.66 }, "dark", 0.021).l).toBe(0.66);
		expect(clampAccent({ h: 40, s: 0.5, l: 0.95 }, "dark", 0.021).l).toBe(0.7);
	});

	it("lifts a saturated blue past the old band on a light bar", () => {
		// HSL 55% blue gave 1.9:1 on Coastal Mist's bar; lightness is measured now.
		const blue = clampAccent({ h: 240, s: 0.85, l: 0.55 }, "dark", 0.0588);
		expect(accentContrast(blue, 0.0588)).toBeGreaterThanOrEqual(4.5);
	});

	it("keeps the light-mode band", () => {
		expect(clampAccent({ h: 200, s: 0.9, l: 0.95 }, "light").l).toBe(0.45);
		expect(clampAccent({ h: 200, s: 0.9, l: 0.1 }, "light").l).toBe(0.3);
	});

	it("emits the bar's control tokens with a readable foreground", () => {
		const dark = accentCssVariables({ h: 200, s: 0.6, l: 0.65 }, "dark");
		expect(dark["--player-control-primary"]).toBe("hsl(200 60% 65%)");
		// Ink on the accent takes the accent's hue.
		expect(dark["--player-control-primary-foreground"]).toBe("hsl(200 30% 8%)");
		const light = accentCssVariables({ h: 200, s: 0.6, l: 0.35 }, "light");
		expect(light["--player-control-primary-foreground"]).toBe(
			"hsl(200 30% 98%)",
		);
		expect(accentCssVariables(null, "dark")).toEqual({});
	});

	it("picks the ink that reads stronger, not a lightness cut-off", () => {
		// At l 0.55 white gave the queue badge's text only 3.4:1.
		const mid = accentCssVariables({ h: 21, s: 0.35, l: 0.55 }, "dark");
		expect(mid["--player-control-primary-foreground"]).toBe("hsl(21 30% 8%)");
	});
});
