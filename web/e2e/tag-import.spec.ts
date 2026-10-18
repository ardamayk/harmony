import { createHash } from "node:crypto";
import { expect, test } from "@playwright/test";
import { buildStrictMp3 } from "./fixtures/managed-import-audio";

const PNG = Buffer.from(
	"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
	"base64",
);

test("new Album cover and file alternatives lead to an explicit batch Track Replacement", async ({
	page,
	request,
}) => {
	const album = `Tag review ${Date.now()}`;
	const tags = {
		title: "Straße",
		artists: ["First Artist"],
		albumArtist: "Album Artist",
		album,
		track: "1",
		genres: [],
	};
	const original = buildStrictMp3(tags, { omitArtwork: true });
	const alternative = buildStrictMp3(
		{ ...tags, artists: ["Selected Artist"] },
		{ omitArtwork: true },
	);
	await page.goto("/library/tracks");
	await page.getByRole("button", { name: "Import Music" }).click();
	const dialog = page.getByRole("dialog", { name: "Import Music" });
	await dialog.getByLabel("Audio files").setInputFiles([
		{ name: "first.mp3", mimeType: "audio/mpeg", buffer: original },
		{ name: "selected.mp3", mimeType: "audio/mpeg", buffer: alternative },
	]);
	await expect(dialog.getByText("2 of 2 ready", { exact: true })).toBeVisible();
	await expect(
		dialog.getByRole("button", { name: "Confirm Import" }),
	).toBeDisabled();
	await dialog.getByRole("radio", { name: /selected.mp3/ }).check();
	await dialog
		.getByLabel(`Upload cover for ${album}`)
		.setInputFiles({ name: "cover.png", mimeType: "image/png", buffer: PNG });
	await expect(dialog.getByText("Uploading artwork…")).toBeHidden();
	const uploadedCover = dialog.getByRole("img", { name: "Uploaded cover 1" });
	await expect(uploadedCover).toBeVisible();
	await expect
		.poll(() =>
			uploadedCover.evaluate((image: HTMLImageElement) => image.naturalWidth),
		)
		.toBeGreaterThan(0);
	await expect(
		dialog.getByRole("radio", { name: `Uploaded cover 1 for ${album}` }),
	).toBeChecked();
	await expect(
		dialog.getByRole("button", { name: "Confirm Import" }),
	).toBeEnabled();
	await dialog.getByRole("button", { name: "Confirm Import" }).click();
	await expect(dialog.getByText("Imported", { exact: true })).toBeVisible();
	await dialog.getByRole("button", { name: "Done" }).click();
	const library = await (
		await request.get("/api/v1/library/tracks", { params: { q: album } })
	).json();
	const track = library.items[0];
	expect(library.items).toHaveLength(1);
	const cover = await request.get(
		`/api/v1/library/albums/${track.albumId}/cover`,
	);
	expect(cover.ok()).toBe(true);
	expect(
		createHash("sha256")
			.update(await cover.body())
			.digest("hex"),
	).toBe(createHash("sha256").update(PNG).digest("hex"));

	const replacement = buildStrictMp3(
		{
			...tags,
			title: "STRASSE",
			artists: ["Updated Artist"],
			genres: ["Ambient"],
		},
		{ omitArtwork: true },
	);
	await page.getByRole("button", { name: "Import Music" }).click();
	await dialog.getByLabel("Audio files").setInputFiles({
		name: "replacement.mp3",
		mimeType: "audio/mpeg",
		buffer: replacement,
	});
	await expect(
		dialog.getByText("Track Replacement", { exact: true }),
	).toBeVisible();
	await dialog
		.getByRole("combobox", { name: `Album destination for ${album}` })
		.click();
	await page
		.getByRole("option", { name: "Create separate album", exact: true })
		.click();
	await expect(
		dialog.getByRole("radio", { name: "Replace existing Track" }),
	).toHaveCount(0);
	await dialog
		.getByRole("combobox", { name: `Album destination for ${album}` })
		.click();
	await page.getByRole("option", { name: /Add to existing album/ }).click();
	await expect(
		dialog.getByText("Existing Album cover will be preserved."),
	).toBeVisible();
	const replace = dialog.getByRole("radio", { name: "Replace existing Track" });
	await expect(replace).toBeEnabled();
	await dialog.getByText(/Review metadata and audio changes/).click();
	await expect(
		dialog.getByRole("cell", { name: "Ambient", exact: true }),
	).toBeVisible();
	await replace.check();
	await dialog.getByRole("button", { name: "Confirm Import" }).click();
	await expect(dialog.getByText("Replaced", { exact: true })).toBeVisible();
	const after = await (
		await request.get("/api/v1/library/tracks", { params: { q: album } })
	).json();
	expect(after.items).toHaveLength(1);
	expect(after.items[0].id).toBe(track.id);
	const bytes = await (
		await request.get(`/api/v1/tracks/${track.id}/stream`)
	).body();
	expect(bytes.equals(replacement)).toBe(true);
});
