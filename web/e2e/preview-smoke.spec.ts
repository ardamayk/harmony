import { expect, test } from "@playwright/test";

// The Music Server sign-in of previews: the public demo account by default.
// PREVIEW_USERNAME and PREVIEW_PASSWORD are Traefik's basic auth instead
// (playwright.preview.config.ts).
const username = process.env.PREVIEW_SIGN_IN_USERNAME || "demo";
const password = process.env.PREVIEW_SIGN_IN_PASSWORD || "demo";

test.skip(!process.env.PREVIEW_URL, "PREVIEW_URL is not set");

test("signs in to a preview deployment and signs out again", async ({
	page,
}) => {
	await page.goto("/");
	await page.getByLabel("Username", { exact: true }).fill(username);
	await page.getByLabel("Password", { exact: true }).fill(password);
	await page.getByRole("button", { name: "Sign in", exact: true }).click();

	await expect(page.getByRole("link", { name: "Albums" })).toBeVisible();
	expect(
		await page.evaluate(async () => (await fetch("/api/v1/me")).status),
	).toBe(200);

	// Sign out lives in Settings → Account.
	await page.goto("/settings");
	await page.getByRole("button", { name: "Sign out", exact: true }).click();

	await expect(
		page.getByRole("button", { name: "Sign in", exact: true }),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "Albums" })).toHaveCount(0);
});
