import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vite-plus/test";

// Enforces the checkable rules in docs/design-rules.md across both UI source
// trees: corner radii, theme color classes over raw variables, and the type
// and spacing scales.
const webSource = import.meta.dirname;
const roots = [webSource, join(webSource, "../../packages/ui/src")];
const repoRoot = join(webSource, "../..");

function sourceFiles(directory: string): string[] {
	return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
		const path = join(directory, entry.name);
		if (entry.isDirectory()) return sourceFiles(path);
		return /\.(tsx?|css)$/.test(entry.name) &&
			!entry.name.includes("routeTree.gen")
			? [path]
			: [];
	});
}

const ROUNDED_CLASS =
	/(?<![\w-])rounded(?:-(?:tl|tr|bl|br|t|b|l|r|s|e))?(?:-([\w[\].]+))?(?=[\s"'`}]|$)/g;
const ALLOWED_ROUNDED_SIZES = new Set(["sm", "md", "lg", "full", "none"]);
const ALLOWED_CSS_RADII = /^(?:0|9999px|var\(--radius-(?:sm|md|lg)\))$/;

function radiusViolations(path: string, source: string): string[] {
	const where = relative(repoRoot, path);
	const violations: string[] = [];
	if (path.endsWith(".css")) {
		for (const match of source.matchAll(/border-radius:\s*([^;]+);/g)) {
			if (!ALLOWED_CSS_RADII.test(match[1].trim()))
				violations.push(`${where}: border-radius: ${match[1]}`);
		}
		return violations;
	}
	// Only class strings: string and template literals.
	for (const literal of source.matchAll(/"[^"\n]*"|`[^`]*`|'[^'\n]*'/g)) {
		for (const match of literal[0].matchAll(ROUNDED_CLASS)) {
			if (!ALLOWED_ROUNDED_SIZES.has(match[1] ?? ""))
				violations.push(`${where}: ${match[0]}`);
		}
	}
	return violations;
}

// A color utility written as `bg-[var(--x)]` while `--color-x` is a theme
// token should be the token class (`bg-x`).
const ARBITRARY_COLOR_VARIABLE =
	/(?<![\w-])(?:bg|text|border(?:-[trblxyse])?|ring|ring-offset|outline|fill|stroke|decoration|from|via|to|caret|accent|divide|placeholder)-\[var\(--([a-z0-9-]+)\)\]/g;
const themeColorTokens = new Set(
	[
		...readFileSync(join(webSource, "styles.css"), "utf8").matchAll(
			/--color-([a-z0-9-]+):/g,
		),
	].map((match) => match[1]),
);

function colorTokenViolations(path: string, source: string): string[] {
	if (path.endsWith(".css")) return [];
	return [...source.matchAll(ARBITRARY_COLOR_VARIABLE)]
		.filter((match) => themeColorTokens.has(match[1]))
		.map((match) => `${relative(repoRoot, path)}: ${match[0]}`);
}

// Classes inside string and template literals, with their variant prefixes
// (`hover:`, `sm:`) stripped.
function classTokens(source: string): string[] {
	return [...source.matchAll(/"[^"\n]*"|`[^`]*`|'[^'\n]*'/g)].flatMap(
		(literal) =>
			literal[0]
				.slice(1, -1)
				.split(/\s+/)
				.map((token) => token.slice(token.lastIndexOf(":") + 1)),
	);
}

const OFF_SCALE_TEXT =
	/^text-(?:lg|4xl|5xl|6xl|7xl|8xl|9xl|\[[\d.]+(?:px|rem|em)\])$/;
const OFF_SCALE_WEIGHT =
	/^font-(?:thin|extralight|light|bold|extrabold|black)$/;
const SPACING =
	/^-?(?:gap-x|gap-y|space-x|space-y|px|py|pt|pb|pl|pr|ps|pe|mx|my|mt|mb|ml|mr|ms|me|gap|p|m)-(.+)$/;
const SPACING_STEPS = new Set([
	"0",
	"px",
	"0.5",
	"1",
	"2",
	"3",
	"4",
	"6",
	"8",
	"12",
	"16",
	"24",
	"auto",
]);

function scaleViolations(path: string, source: string): string[] {
	if (path.endsWith(".css")) return [];
	const where = relative(repoRoot, path);
	return classTokens(source).flatMap((token) => {
		if (OFF_SCALE_TEXT.test(token) || OFF_SCALE_WEIGHT.test(token))
			return [`${where}: ${token}`];
		const spacing = SPACING.exec(token);
		if (!spacing) return [];
		const step = spacing[1];
		// Layout geometry tied to a CSS variable is allowed (`px-[var(--shell-inset)]`).
		if (
			SPACING_STEPS.has(step) ||
			/^\[var\(--[\w-]+(?:,[^\]]+)?\)\]$/.test(step)
		)
			return [];
		return [`${where}: ${token}`];
	});
}

describe("design rules", () => {
	it("uses only the sm, md and lg corner radii (plus full and none)", () => {
		const violations = roots
			.flatMap(sourceFiles)
			.filter((path) => !path.endsWith("design-rules.test.ts"))
			.flatMap((path) => radiusViolations(path, readFileSync(path, "utf8")));
		expect(violations).toEqual([]);
	});

	it("uses theme color classes instead of raw variables that have a token", () => {
		const violations = roots
			.flatMap(sourceFiles)
			.filter((path) => !path.endsWith("design-rules.test.ts"))
			.flatMap((path) =>
				colorTokenViolations(path, readFileSync(path, "utf8")),
			);
		expect(violations).toEqual([]);
	});

	it("uses only the type scale, three weights and the spacing scale", () => {
		const violations = roots
			.flatMap(sourceFiles)
			.filter((path) => !/(design-rules|\.test)\.tsx?$/.test(path))
			.flatMap((path) => scaleViolations(path, readFileSync(path, "utf8")));
		expect(violations).toEqual([]);
	});

	it("requests album covers at a display size", () => {
		const unsized = /getAlbumCoverUrl\(\s*[^,()]+\s*\)/g;
		const violations = roots
			.flatMap(sourceFiles)
			.filter((path) => !/(design-rules|\.test)\.tsx?$/.test(path))
			.flatMap((path) =>
				[...readFileSync(path, "utf8").matchAll(unsized)].map(
					(match) => `${relative(repoRoot, path)}: ${match[0]}`,
				),
			);
		expect(violations).toEqual([]);
	});
});
