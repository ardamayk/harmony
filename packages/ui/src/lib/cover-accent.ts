/**
 * Cover-driven accent colour. The dominant saturated hue of an album cover
 * tints the Player Bar's primary control; lightness is clamped per theme so
 * the result always keeps contrast against the bar and its foreground.
 */
export type Accent = {
	h: number;
	s: number;
	l: number;
	/** No hue at all: neutral controls, the cover's mean colour as backdrop tint. */
	muted?: boolean;
	/** A pale colour the soft pixels agree on; it stays pale on the controls. */
	soft?: boolean;
	/** The next hue buckets by weight, for a theme that cannot take the winner. */
	runnersUp?: Array<{ h: number; s: number; l: number }>;
};

export type AccentThemeMode = "light" | "dark";

const SAMPLE_SIZE = 32;
const MIN_SATURATION = 0.2;
const MIN_LIGHTNESS = 0.12;
const MAX_LIGHTNESS = 0.9;
// Below this a pixel's hue is noise; it does not vote.
const HUE_VOTE_SATURATION = 0.03;
// A soft cover colour counts when this share of pixels agrees on one hue.
const MIN_HUE_SHARE = 0.3;
const MIN_HUE_AGREEMENT = 0.85;
// A soft accent is lifted only this far, so a pale lilac does not turn purple.
const MAX_SOFT_SATURATION = 0.2;
// Below this share of saturated pixels the colour is a detail (an eye on a
// white cover), not the cover's colour, so it does not set the accent.
const MIN_COLOUR_SHARE = 0.15;

const ACCENT_LIGHTNESS_RANGE: Record<AccentThemeMode, [number, number]> = {
	dark: [0.55, 0.7],
	light: [0.3, 0.45],
};
// Dark mode lifts the accent only until it reads on the bar at the 3:1 a
// control needs: the cover accent never sets text, only the play disc, seek
// fill and active icons (ink on it is chosen separately). HSL lightness is
// not perceived lightness (a saturated blue at 55% is darker than a teal at
// 45%), so the lift is measured, not a fixed band. 3.1, not 3: the browser
// rounds the colour to 8 bits per channel, which can cost a few hundredths.
const ACCENT_MIN_CONTRAST = 3.1;
// Vintage Harbor's navy; a cover hue this close to it would make the lit
// surface read as the neutral active step, so the theme's peach stays.
const BASE_HUE = 252;
const BASE_HUE_GUARD = 30;
// Vintage Harbor's accent fill (the peach): OKLCH L 83, C 0.063. A cover's
// colour scales the C down (paler cover) and the L down (darker cover), to
// the lowest L whose fill keeps 3:1 against the bar and the lit step and
// 4.5:1 under its label at every hue.
const SLOT_CHROMA = 0.063;
const MIN_CHROMA_SCALE = 0.3;
const SLOT_LIGHTNESS = 83;
const MIN_FILL_LIGHTNESS = 64;
const COVER_LIGHTNESS_LIFT = 20;
const DARK_ACCENT_CEILING = 0.7;
// Vintage Harbor's dark Player Bar, for callers that cannot read the bar.
const DEFAULT_DARK_SURFACE_LUMINANCE = 0.021;

export function rgbToHsl(r: number, g: number, b: number): Accent {
	const red = r / 255;
	const green = g / 255;
	const blue = b / 255;
	const max = Math.max(red, green, blue);
	const min = Math.min(red, green, blue);
	const l = (max + min) / 2;
	if (max === min) return { h: 0, s: 0, l };
	const d = max - min;
	const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
	let h: number;
	if (max === red) h = (green - blue) / d + (green < blue ? 6 : 0);
	else if (max === green) h = (blue - red) / d + 2;
	else h = (red - green) / d + 4;
	return { h: (h / 6) * 360, s, l };
}

function srgbToOklch(r: number, g: number, b: number) {
	const [red, green, blue] = [r, g, b].map((channel) => {
		const c = channel / 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});
	const l = Math.cbrt(
		0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue,
	);
	const m = Math.cbrt(
		0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue,
	);
	const s = Math.cbrt(
		0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue,
	);
	const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
	const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
	const h = (Math.atan2(bb, a) * 180) / Math.PI;
	return {
		l: 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
		c: Math.hypot(a, bb),
		h: h < 0 ? h + 360 : h,
	};
}

/** The hue the theme's accent may take: none near the navy base. */
export function themeAccentHue(hue: number | undefined): number | undefined {
	if (hue === undefined) return undefined;
	const distance = Math.abs(((hue - BASE_HUE + 540) % 360) - 180);
	return distance <= BASE_HUE_GUARD ? undefined : hue;
}

/**
 * Picks the accent from raw RGBA pixels: the hue bucket with the most
 * saturated weight wins, and the accent is that bucket's weighted mean.
 * Without saturated pixels, soft pixels that agree on one hue (a pale
 * tinted backdrop) still give an accent; otherwise the cover's mean colour
 * comes back marked muted, and null only when every pixel is near black
 * or white.
 */
export function accentFromPixels(data: Uint8ClampedArray): Accent | null {
	const buckets = new Map<
		number,
		{ weight: number; h: number; s: number; l: number }
	>();
	let opaque = 0;
	let coloured = 0;
	for (let index = 0; index + 3 < data.length; index += 4) {
		if (data[index + 3] < 128) continue;
		opaque += 1;
		const { h, s, l } = rgbToHsl(data[index], data[index + 1], data[index + 2]);
		if (s < MIN_SATURATION || l < MIN_LIGHTNESS || l > MAX_LIGHTNESS) continue;
		coloured += 1;
		const bucket = Math.floor(h / 30);
		const weight = s * (1 - Math.abs(l - 0.5));
		const entry = buckets.get(bucket) ?? { weight: 0, h: 0, s: 0, l: 0 };
		entry.weight += weight;
		entry.h += h * weight;
		entry.s += s * weight;
		entry.l += l * weight;
		buckets.set(bucket, entry);
	}
	const ranked = [...buckets.values()]
		.filter((entry) => entry.weight > 0)
		.sort((a, b) => b.weight - a.weight)
		.map((entry) => ({
			h: entry.h / entry.weight,
			s: entry.s / entry.weight,
			l: entry.l / entry.weight,
		}));
	if (ranked.length === 0) return agreedSoftHue(data) ?? mutedFromPixels(data);
	// A small coloured detail on a colourless cover: colourless, not soft.
	if (coloured < opaque * MIN_COLOUR_SHARE) return mutedFromPixels(data);
	const [best, ...runnersUp] = ranked;
	return runnersUp.length ? { ...best, runnersUp } : best;
}

/**
 * Soft pixels' hues as unit vectors: their mean's length says how far they
 * agree (1 = one hue, near 0 = scattered noise), its angle is the hue.
 */
function agreedSoftHue(data: Uint8ClampedArray): Accent | null {
	let counted = 0;
	let votes = 0;
	let x = 0;
	let y = 0;
	let saturation = 0;
	let lightness = 0;
	for (let index = 0; index + 3 < data.length; index += 4) {
		if (data[index + 3] < 128) continue;
		const { h, s, l } = rgbToHsl(data[index], data[index + 1], data[index + 2]);
		if (l < MIN_LIGHTNESS || l > MAX_LIGHTNESS) continue;
		counted += 1;
		if (s < HUE_VOTE_SATURATION) continue;
		votes += 1;
		const angle = (h * Math.PI) / 180;
		x += Math.cos(angle);
		y += Math.sin(angle);
		saturation += s;
		lightness += l;
	}
	if (votes === 0 || votes / counted < MIN_HUE_SHARE) return null;
	if (Math.hypot(x, y) / votes < MIN_HUE_AGREEMENT) return null;
	const hue = (Math.atan2(y, x) * 180) / Math.PI;
	return {
		h: hue < 0 ? hue + 360 : hue,
		s: saturation / votes,
		l: lightness / votes,
		soft: true,
	};
}

function mutedFromPixels(data: Uint8ClampedArray): Accent | null {
	let count = 0;
	let red = 0;
	let green = 0;
	let blue = 0;
	for (let index = 0; index + 3 < data.length; index += 4) {
		if (data[index + 3] < 128) continue;
		const { l } = rgbToHsl(data[index], data[index + 1], data[index + 2]);
		if (l < MIN_LIGHTNESS || l > MAX_LIGHTNESS) continue;
		count += 1;
		red += data[index];
		green += data[index + 1];
		blue += data[index + 2];
	}
	if (count === 0) return null;
	return {
		...rgbToHsl(red / count, green / count, blue / count),
		muted: true,
	};
}

/**
 * Clamps the accent so it reads on the theme's bar. Dark mode keeps the
 * cover's own lightness (up to a ceiling) and lifts it just far enough for
 * 3:1 against the bar; light mode keeps its fixed band.
 */
export function clampAccent(
	accent: Accent,
	mode: AccentThemeMode,
	surfaceLuminance = DEFAULT_DARK_SURFACE_LUMINANCE,
): Accent {
	const s = accent.soft
		? Math.min(MAX_SOFT_SATURATION, accent.s * 2)
		: Math.min(0.85, Math.max(0.35, accent.s));
	if (mode === "light") {
		const [min, max] = ACCENT_LIGHTNESS_RANGE.light;
		return { h: accent.h, s, l: Math.min(max, Math.max(min, accent.l)) };
	}
	let l = Math.min(DARK_ACCENT_CEILING, accent.l);
	while (
		accentContrast({ h: accent.h, s, l }, surfaceLuminance) <
			ACCENT_MIN_CONTRAST &&
		l < 0.95
	) {
		l = Math.round((l + 0.01) * 100) / 100;
	}
	return { h: accent.h, s, l };
}

/** WCAG contrast of the accent against a surface's relative luminance. */
export function accentContrast(accent: Accent, surfaceLuminance: number) {
	const [light, dark] = [relativeLuminance(accent), surfaceLuminance].sort(
		(x, y) => y - x,
	);
	return (light + 0.05) / (dark + 0.05);
}

function hslToRgb({ h, s, l }: Accent): [number, number, number] {
	const k = (n: number) => (n + h / 30) % 12;
	const a = s * Math.min(l, 1 - l);
	const f = (n: number) =>
		l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
	return [f(0), f(8), f(4)];
}

function relativeLuminance(accent: Accent): number {
	const [r, g, b] = hslToRgb(accent).map((c) =>
		c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4,
	);
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrastRatio(a: Accent, b: Accent): number {
	const [light, dark] = [relativeLuminance(a), relativeLuminance(b)].sort(
		(x, y) => y - x,
	);
	return (light + 0.05) / (dark + 0.05);
}

/**
 * Text and glyphs on the accent belong to the accent's own family: a near
 * black or near white of its hue (neutral for a grey accent), whichever
 * reads stronger on it.
 */
function inkOn(accent: Accent): Accent {
	const s = Math.min(0.3, accent.s);
	const dark = { h: accent.h, s, l: 0.08 };
	const light = { h: accent.h, s, l: 0.98 };
	return contrastRatio(dark, accent) >= contrastRatio(light, accent)
		? dark
		: light;
}

function accentToCss(accent: Accent): string {
	return `hsl(${accent.h.toFixed(0)} ${(accent.s * 100).toFixed(0)}% ${(accent.l * 100).toFixed(0)}%)`;
}

/** CSS custom properties the Player Bar reads; empty when there is no accent. */
export function accentCssVariables(
	accent: Accent | null,
	mode: AccentThemeMode,
	surfaceLuminance?: number,
): Record<string, string> {
	if (!accent) return {};
	const [min, max] = ACCENT_LIGHTNESS_RANGE[mode];
	// A colourless cover gets neutral controls, at the band's end farthest
	// from the bar; its backdrop keeps the cover's own faint mean colour.
	const clamped = accent.muted
		? clampAccent(
				{ h: 0, s: 0, l: mode === "dark" ? max : min, soft: true },
				mode,
				surfaceLuminance,
			)
		: clampAccent(accent, mode, surfaceLuminance);
	const color = accentToCss(clamped);
	const tint = accent.muted
		? accentToCss({
				h: accent.h,
				s: accent.s,
				l: Math.min(max, Math.max(min, accent.l)),
			})
		: color;
	const foreground = accentToCss(inkOn(clamped));
	// The control tokens are set directly on the bar element so a theme
	// preset that hard-codes them at :root cannot mask the accent.
	return {
		"--player-accent": color,
		"--cover-tint": tint,
		"--player-accent-foreground": foreground,
		"--player-control-primary": color,
		"--player-control-primary-foreground": foreground,
		"--player-live-progress": color,
		"--player-control-shadow": `hsl(${clamped.h.toFixed(0)} ${(clamped.s * 100).toFixed(0)}% ${(clamped.l * 100).toFixed(0)}% / 0.25)`,
		...accentHueVariables(accent),
	};
}

/**
 * What a hue-driven theme (Vintage Harbor) takes from the accent: the picked
 * colour's OKLCH hue, its own L and C staying the theme's. A colourless cover
 * drops the chroma to 0 (white-grey controls); a hue near the navy base keeps
 * the theme's own.
 */
export function accentHueVariables(accent: Accent): Record<string, string> {
	if (accent.muted) return { "--accent-chroma": "0" };
	// The first colour, by weight, the theme can take: a blue sky winning on a
	// pink cover gives way to the pink.
	const picked = [accent, ...(accent.runnersUp ?? [])]
		.map((colour) => {
			const [r, g, b] = hslToRgb(colour).map((channel) => channel * 255);
			return srgbToOklch(r, g, b);
		})
		.find((colour) => themeAccentHue(colour.h) !== undefined);
	if (!picked) return {};
	const { l, c, h: hue } = picked;
	const clamp = (value: number, min: number, max: number) =>
		Math.min(max, Math.max(min, value));
	return {
		"--accent-hue": hue.toFixed(1),
		"--accent-chroma": clamp(c / SLOT_CHROMA, MIN_CHROMA_SCALE, 1).toFixed(2),
		"--accent-lightness": clamp(
			l * 100 + COVER_LIGHTNESS_LIFT,
			MIN_FILL_LIGHTNESS,
			SLOT_LIGHTNESS,
		).toFixed(1),
	};
}

/**
 * Draws the image onto a small canvas and extracts the accent. Returns
 * null when drawing fails, including the tainted-canvas case where the
 * image was served without CORS headers.
 */
export function extractAccent(image: HTMLImageElement): Accent | null {
	try {
		const canvas = document.createElement("canvas");
		canvas.width = SAMPLE_SIZE;
		canvas.height = SAMPLE_SIZE;
		const context = canvas.getContext("2d", { willReadFrequently: true });
		if (!context) return null;
		context.drawImage(image, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
		return accentFromPixels(
			context.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE).data,
		);
	} catch {
		return null;
	}
}
