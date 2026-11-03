import { defineConfig, devices } from "@playwright/test";

// Runs against a Dokploy preview deployment (docs/deploy-dokploy.md); the
// Preview E2E workflow sets PREVIEW_URL, so there is no webServer here.
export default defineConfig({
	testDir: "./e2e",
	testMatch: "preview-smoke.spec.ts",
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	workers: 1,
	// A remote server behind a tunnel answers slower than a local one.
	timeout: 60_000,
	expect: { timeout: 15_000 },
	retries: process.env.CI ? 1 : 0,
	reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
	use: {
		baseURL: process.env.PREVIEW_URL,
		// Dokploy's Security tab puts Traefik basic auth in front of previews,
		// with the same credentials as the Music Server's sign-in.
		httpCredentials: {
			username: process.env.PREVIEW_USERNAME ?? "",
			password: process.env.PREVIEW_PASSWORD ?? "",
		},
		trace: "on-first-retry",
		screenshot: "only-on-failure",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
