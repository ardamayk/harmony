import { defineConfig, devices } from "@playwright/test";

const PRODUCTION_API_PORT = 18090;
const PRODUCTION_API_ORIGIN = `http://127.0.0.1:${PRODUCTION_API_PORT}`;

export default defineConfig({
	testDir: "./e2e",
	testMatch: "production-smoke.spec.ts",
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	workers: 1,
	timeout: 15_000,
	expect: { timeout: 5_000 },
	// CI diagnostics for the Integration Gate: one retry, and on failure an
	// HTML report plus traces, screenshots, and retry data (issue #79).
	retries: process.env.CI ? 1 : 0,
	reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
	use: {
		baseURL: "http://127.0.0.1:4173",
		trace: "on-first-retry",
		screenshot: "only-on-failure",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	webServer: [
		{
			// The first run compiles the Music Server.
			command: "cargo run --quiet -p earthly-music-server",
			timeout: 600_000,
			url: `${PRODUCTION_API_ORIGIN}/api/v1/health`,
			cwd: "../server",
			env: {
				SERVER_ADDR: `127.0.0.1:${PRODUCTION_API_PORT}`,
				DATABASE_PATH: "./data/e2e-production.db",
			},
			reuseExistingServer: false,
		},
		{
			command:
				"pnpm run build && pnpm run preview --host 127.0.0.1 --port 4173",
			timeout: 120_000,
			url: "http://127.0.0.1:4173",
			env: { VITE_PROXY_TARGET: PRODUCTION_API_ORIGIN },
			reuseExistingServer: false,
		},
	],
});
