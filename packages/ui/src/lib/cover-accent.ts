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

const ACCENT_LIGHTNESS_RANGE: Record<AccentThemeMode, [number, number]> = {
	dark: [0.55, 0.7],
	light: [0.3, 0.45],
};

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
	for (let index = 0; index + 3 < data.length; index += 4) {
		if (data[index + 3] < 128) continue;
		const { h, s, l } = rgbToHsl(data[index], data[index + 1], data[index + 2]);
		if (s < MIN_SATURATION || l < MIN_LIGHTNESS || l > MAX_LIGHTNESS) continue;
		const bucket = Math.floor(h / 30);
		const weight = s * (1 - Math.abs(l - 0.5));
		const entry = buckets.get(bucket) ?? { weight: 0, h: 0, s: 0, l: 0 };
		entry.weight += weight;
		entry.h += h * weight;
		entry.s += s * weight;
		entry.l += l * weight;
		buckets.set(bucket, entry);
	}
	let best: { weight: number; h: number; s: number; l: number } | null = null;
	for (const entry of buckets.values()) {
		if (!best || entry.weight > best.weight) best = entry;
	}
	if (!best || best.weight === 0)
		return agreedSoftHue(data) ?? mutedFromPixels(data);
	return {
		h: best.h / best.weight,
		s: best.s / best.weight,
		l: best.l / best.weight,
	};
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

/** Clamps the accent into the theme's readable lightness band. */
export function clampAccent(accent: Accent, mode: AccentThemeMode): Accent {
	const [min, max] = ACCENT_LIGHTNESS_RANGE[mode];
	return {
		h: accent.h,
		s: accent.soft
			? Math.min(MAX_SOFT_SATURATION, accent.s * 2)
			: Math.min(0.85, Math.max(0.35, accent.s)),
		l: Math.min(max, Math.max(min, accent.l)),
	};
}

function accentToCss(accent: Accent): string {
	return `hsl(${accent.h.toFixed(0)} ${(accent.s * 100).toFixed(0)}% ${(accent.l * 100).toFixed(0)}%)`;
}

/** CSS custom properties the Player Bar reads; empty when there is no accent. */
export function accentCssVariables(
	accent: Accent | null,
	mode: AccentThemeMode,
): Record<string, string> {
	if (!accent) return {};
	const [min, max] = ACCENT_LIGHTNESS_RANGE[mode];
	// A colourless cover gets neutral controls, at the band's end farthest
	// from the bar; its backdrop keeps the cover's own faint mean colour.
	const clamped = accent.muted
		? { h: 0, s: 0, l: mode === "dark" ? max : min }
		: clampAccent(accent, mode);
	const color = accentToCss(clamped);
	const tint = accent.muted
		? accentToCss({
				h: accent.h,
				s: accent.s,
				l: Math.min(max, Math.max(min, accent.l)),
			})
		: color;
	// Text on the accent: dark on light accents, light on dark ones.
	const foreground = clamped.l > 0.55 ? "#111111" : "#ffffff";
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
