import { expect, test } from "@playwright/test";

const username = process.env.PREVIEW_USERNAME ?? "";
const password = process.env.PREVIEW_PASSWORD ?? "";

test.skip(!process.env.PREVIEW_URL, "PREVIEW_URL is not set");

test("signs in to a preview deployment and signs out again", async ({
	page,
}) => {
	expect(username, "PREVIEW_USERNAME is not set").not.toBe("");
	expect(password, "PREVIEW_PASSWORD is not set").not.toBe("");

	await page.goto("/");
	await page.getByLabel("Username", { exact: true }).fill(username);
	await page.getByLabel("Password", { exact: true }).fill(password);
	await page.getByRole("button", { name: "Sign in", exact: true }).click();

	await expect(page.getByRole("link", { name: "Albums" })).toBeVisible();
	expect(
		await page.evaluate(async () => (await fetch("/api/v1/me")).status),
	).toBe(200);

	await page.getByRole("button", { name: "Sign out", exact: true }).click();

	await expect(
		page.getByRole("button", { name: "Sign in", exact: true }),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "Albums" })).toHaveCount(0);
});
