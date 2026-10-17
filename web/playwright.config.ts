import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
	testDir: "./e2e",
	testIgnore: ["radio-hls-proxy.spec.ts", "production-smoke.spec.ts"],
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	// CI diagnostics for the Integration Gate: one retry, and on failure an
	// HTML report plus traces, screenshots, and retry data (issue #79).
	retries: process.env.CI ? 1 : 0,
	// Import journeys inspect the same server staging directory.
	workers: 1,
	reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
	use: {
		baseURL: process.env.PLAYWRIGHT_BASE_URL ?? "http://localhost:3000",
		trace: "on-first-retry",
		screenshot: "only-on-failure",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	// Always start fresh servers on the isolated E2E database and storage: a
	// running development server would receive the test imports instead.
	webServer: [
		{
			// The first run compiles the Music Server.
			command: "cargo run --quiet -p earthly-music-server",
			timeout: 600_000,
			url: "http://localhost:8090/api/v1/health",
			reuseExistingServer: false,
			cwd: "../server",
			// Pass the same isolated path to the server and staging assertions.
			env: {
				SERVER_ADDR: "127.0.0.1:8090",
				DATABASE_PATH: "./data/e2e.db",
				MANAGED_STORAGE_PATH: "./data/e2e-managed",
			},
		},
		{
			command: "pnpm run dev",
			timeout: 120_000,
			url: "http://localhost:3000",
			reuseExistingServer: false,
		},
	],
});
