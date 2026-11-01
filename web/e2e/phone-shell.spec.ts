import { expect, type Page, test } from "@playwright/test";
import { buildStrictMp3 } from "./fixtures/managed-import-audio";

const PHONE = { width: 402, height: 874 };

/** Imports one Track through the desktop shell and returns its Album's id. */
async function importAlbum(page: Page): Promise<string> {
	const album = `Phone shell ${Date.now()}`;
	await page.goto("/settings");
	await page.getByRole("button", { name: "Import Music" }).click();
	const dialog = page.getByRole("dialog", { name: "Import Music" });
	await dialog.getByLabel("Audio files").setInputFiles({
		name: "phone.mp3",
		mimeType: "audio/mpeg",
		buffer: buildStrictMp3({
			title: "Pocket song",
			artists: ["Pocket Artist"],
			albumArtist: "Pocket Artist",
			album,
			track: "1",
			genres: [],
		}),
	});
	await expect(dialog.getByText("1 of 1 ready", { exact: true })).toBeVisible();
	await dialog.getByRole("button", { name: "Confirm Import" }).click();
	await expect(dialog.getByText("Imported", { exact: true })).toBeVisible();
	await dialog.getByRole("button", { name: "Done" }).click();
	const library = await (
		await page.request.get("/api/v1/library/tracks", { params: { q: album } })
	).json();
	return library.items[0].albumId;
}

async function expectNoHorizontalOverflow(page: Page) {
	expect(
		await page.evaluate(
			() =>
				document.documentElement.scrollWidth <=
				document.documentElement.clientWidth,
		),
	).toBe(true);
}

test("phone shell navigates, plays and opens Now Playing and the Queue", async ({
	page,
}) => {
	// The fixture lasts a moment; ending would take the mini player with it.
	await page.addInitScript(() => {
		HTMLMediaElement.prototype.play = () => Promise.resolve();
	});
	const albumId = await importAlbum(page);
	await page.setViewportSize(PHONE);

	await page.goto("/library/albums");
	await expect(page.locator("[data-phone-shell]")).toBeVisible();
	await expect(page.locator("[data-sidebar]")).toHaveCount(0);
	await expect(page.locator("[data-queue-drawer]")).toHaveCount(0);
	await expectNoHorizontalOverflow(page);

	const tabs = page.getByRole("navigation", { name: "Library" });
	await tabs.getByRole("link", { name: "Artists" }).click();
	await expect(page).toHaveURL(/\/library\/artists/);
	await tabs.getByRole("button", { name: "More" }).click();
	const more = page.getByRole("dialog", { name: "More" });
	await more.getByRole("link", { name: "Genres" }).click();
	await expect(more).toBeHidden();
	await expect(page).toHaveURL(/\/library\/genres/);

	await expect(page.locator("[data-mini-player]")).toHaveCount(0);
	await page.goto(`/library/${albumId}`);
	await page.getByRole("main").getByRole("button", { name: "Play" }).click();
	await page.getByRole("button", { name: /Open now playing/ }).click();
	const nowPlaying = page.getByRole("dialog", { name: "Now playing" });
	await expect(
		nowPlaying.getByRole("heading", { name: "Pocket song" }),
	).toBeVisible();
	await expect(nowPlaying.getByRole("slider", { name: "Seek" })).toBeVisible();

	await nowPlaying.getByRole("button", { name: "Queue" }).click();
	await expect(
		nowPlaying.getByRole("heading", { name: "Queue" }),
	).toBeVisible();
	await expect(
		nowPlaying.getByRole("button", { name: "Pin queue" }),
	).toHaveCount(0);
	await nowPlaying.getByRole("button", { name: "Hide queue" }).click();
	await expect(nowPlaying.getByRole("heading", { name: "Queue" })).toBeHidden();
	await expectNoHorizontalOverflow(page);
});

test("the desktop shell stays at 1280 wide", async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto("/library/albums");
	await expect(page.locator("[data-sidebar]")).toBeVisible();
	await expect(page.locator("[data-queue-drawer]")).toBeAttached();
	await expect(page.getByTestId("player-bar")).toBeVisible();
	await expect(page.locator("[data-phone-shell]")).toHaveCount(0);
});
