import { expect, test } from "@playwright/test";
import { buildStrictMp3 } from "./fixtures/managed-import-audio";

test("import preview explains the destination and displays a selectable embedded cover", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1440, height: 1000 });
	await page.route("**/api/v1/preferences", async (route) => {
		const response = await route.fetch();
		const preferences = await response.json();
		await route.fulfill({
			response,
			json: { ...preferences, theme: { mode: "dark", preset: "tokyo-night" } },
		});
	});
	const album = `Cover review ${Date.now()}`;
	const file = buildStrictMp3({
		title: "First Light",
		artists: ["Test Artist"],
		albumArtist: "Test Artist",
		album,
		track: "1",
		genres: [],
	});
	await page.goto("/library/tracks");
	await page.getByRole("button", { name: "Import Music", exact: true }).click();
	const dialog = page.getByRole("dialog", { name: "Import Music" });
	await dialog.getByLabel("Audio files", { exact: true }).setInputFiles({
		name: "first-light.mp3",
		mimeType: "audio/mpeg",
		buffer: file,
	});
	await expect(dialog.getByText("1 of 1 ready", { exact: true })).toBeVisible();
	await expect(dialog.getByText("Review albums and covers")).toBeVisible();
	await expect(page.locator("html")).toHaveClass(/dark/);
	await expect(
		dialog.getByText(/Nothing is added to your library until you confirm/),
	).toBeVisible();
	await expect(
		dialog.getByText("Create new album", { exact: true }),
	).toBeVisible();
	await expect(
		dialog.getByRole("combobox", { name: `Album destination for ${album}` }),
	).toHaveCount(0);
	const cover = dialog.getByRole("img", { name: "Embedded cover 1" });
	await expect
		.poll(() => cover.evaluate((image: HTMLImageElement) => image.naturalWidth))
		.toBeGreaterThan(0);
	await expect(
		dialog.getByRole("radio", { name: `Embedded cover 1 for ${album}` }),
	).toBeChecked();
	await dialog.getByRole("radio", { name: `No cover for ${album}` }).check();
	await expect(
		dialog.getByRole("radio", { name: `No cover for ${album}` }),
	).toBeChecked();
	await expect(
		dialog.getByRole("button", { name: "Confirm Import" }),
	).toBeEnabled();
	await dialog
		.getByRole("radio", { name: `Embedded cover 1 for ${album}` })
		.check();
	await page.screenshot({
		path: test.info().outputPath("import-cover-desktop.png"),
	});
	await page.setViewportSize({ width: 390, height: 844 });
	await expect(cover).toBeVisible();
	const hasOverflow = await dialog.evaluate(
		(element) => element.scrollWidth > element.clientWidth,
	);
	expect(hasOverflow).toBe(false);
	await expect(
		dialog.getByRole("button", { name: "Confirm Import" }),
	).toBeVisible();
	await page.screenshot({
		path: test.info().outputPath("import-cover-mobile.png"),
	});
	await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
});
