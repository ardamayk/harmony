// Runs Lighthouse (desktop profile) against a running production build and
// fails when a page misses the performance budgets in docs/design-rules.md.
// `mise run web:lighthouse` builds and starts the Music Server first.
//
// Desktop profile only: this UI targets desktop browsers and the Desktop
// Client. Add a mobile-profile run if it ever targets phones.
//
// Lighthouse measures a load without a user, so INP is held through its lab
// stand-in, Total Blocking Time, to the same 200ms.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const BASE_URL = process.env.LIGHTHOUSE_BASE_URL ?? "http://127.0.0.1:8090";
const PAGES = (
	process.env.LIGHTHOUSE_PAGES ??
	"/library/albums,/library/artists,/library/tracks,/playlists,/radio"
).split(",");

const BUDGETS = [
	{ audit: "largest-contentful-paint", label: "LCP", max: 2500, unit: "ms" },
	{ audit: "total-blocking-time", label: "TBT (INP)", max: 200, unit: "ms" },
	{ audit: "cumulative-layout-shift", label: "CLS", max: 0.1, unit: "" },
	{ audit: "server-response-time", label: "TTFB", max: 800, unit: "ms" },
] as const;

// Lighthouse needs a Chrome; fall back to the Chromium Playwright installs.
function chromePath(): string | undefined {
	if (process.env.CHROME_PATH) return process.env.CHROME_PATH;
	const cache = join(homedir(), ".cache/ms-playwright");
	if (!existsSync(cache)) return undefined;
	const chromium = readdirSync(cache)
		.filter((name) => /^chromium-\d+$/.test(name))
		.sort()
		.at(-1);
	const binary = chromium && join(cache, chromium, "chrome-linux64/chrome");
	return binary && existsSync(binary) ? binary : undefined;
}

const chrome = chromePath();
if (!chrome) {
	console.error(
		"No Chrome found: set CHROME_PATH or run `bunx playwright install chromium`.",
	);
	process.exit(1);
}

let failed = false;
for (const page of PAGES) {
	const result = spawnSync(
		join(import.meta.dirname, "../node_modules/.bin/lighthouse"),
		[
			`${BASE_URL}${page}`,
			"--preset=desktop",
			"--only-categories=performance",
			"--output=json",
			"--output-path=stdout",
			"--quiet",
			"--chrome-flags=--headless=new --no-sandbox",
		],
		{
			encoding: "utf8",
			env: { ...process.env, CHROME_PATH: chrome },
			maxBuffer: 64 * 1024 * 1024,
		},
	);
	if (result.status !== 0) {
		console.error(`Lighthouse failed on ${page}:\n${result.stderr}`);
		failed = true;
		continue;
	}
	const report = JSON.parse(result.stdout) as {
		audits: Record<string, { numericValue?: number }>;
	};
	console.log(`\n${page}`);
	for (const { audit, label, max, unit } of BUDGETS) {
		const value = report.audits[audit]?.numericValue;
		const shown =
			value === undefined
				? "n/a"
				: unit
					? `${Math.round(value)}${unit}`
					: value.toFixed(3);
		const over = value !== undefined && value > max;
		if (over) failed = true;
		console.log(
			`  ${over ? "✗" : "✓"} ${label.padEnd(10)} ${shown.padStart(8)}  (budget ${max}${unit})`,
		);
	}
}

process.exit(failed ? 1 : 0);
