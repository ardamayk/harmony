export type HexColor = `#${string}`;

function relativeLuminance(hex: HexColor): number {
	const normalized = hex.toLowerCase();
	const r = Number.parseInt(normalized.slice(1, 3), 16) / 255;
	const g = Number.parseInt(normalized.slice(3, 5), 16) / 255;
	const b = Number.parseInt(normalized.slice(5, 7), 16) / 255;
	const linearize = (channel: number) =>
		channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
	return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b);
}

export function isDarkToLightOrder(colors: readonly HexColor[]): boolean {
	for (let index = 1; index < colors.length; index += 1) {
		if (
			relativeLuminance(colors[index]) + 0.001 <
			relativeLuminance(colors[index - 1])
		) {
			return false;
		}
	}
	return true;
}

export function contrastRatio(
	foreground: HexColor,
	background: HexColor,
): number {
	const a = relativeLuminance(foreground);
	const b = relativeLuminance(background);
	const lighter = Math.max(a, b);
	const darker = Math.min(a, b);
	return (lighter + 0.05) / (darker + 0.05);
}

const PALETTE_VAR_RE = /--theme-([1-6]):\s*(#[0-9a-fA-F]{6})/g;

export function parsePaletteFromCss(css: string): HexColor[] {
	const slots: HexColor[] = [];
	for (const match of css.matchAll(PALETTE_VAR_RE)) {
		const index = Number(match[1]) - 1;
		slots[index] = match[2].toLowerCase() as HexColor;
	}
	if (slots.length !== 6 || slots.some((color) => !color)) {
		throw new Error("Palette CSS must define --theme-1 through --theme-6");
	}
	return slots;
}
