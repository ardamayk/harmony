import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

// Two browser contexts stand for two devices of one user (ADR 0040).
const TITLE = `Shared ${Date.now()}`;

function longFlac(): Buffer {
	const path = join(mkdtempSync(join(tmpdir(), "shared-")), "long.flac");
	execFileSync("ffmpeg", [
		"-loglevel",
		"error",
		"-f",
		"lavfi",
		"-i",
		"sine=frequency=440:duration=120",
		"-metadata",
		`title=${TITLE}`,
		"-metadata",
		"artist=Shared Artist",
		"-metadata",
		"album_artist=Shared Artist",
		"-metadata",
		`album=${TITLE} Album`,
		"-metadata",
		"track=1",
		"-metadata",
		"disc=1/1",
		"-metadata",
		"date=2026-01-01",
		path,
	]);
	return readFileSync(path);
}

const control = (page: Page, name: "Play" | "Pause") =>
	page.getByRole("button", { name, exact: true });
// The engine's audio element is not in the document.
const trackMediaElements = () => {
	window.Audio = new Proxy(window.Audio, {
		construct(target, args: [string?]) {
			const media = new target(...args);
			Object.assign(window, { sharedPlaybackMedia: media });
			return media;
		},
	});
};
const isAudioPlaying = (page: Page) =>
	page.evaluate(() => {
		const { sharedPlaybackMedia: media } = window as unknown as {
			sharedPlaybackMedia?: HTMLMediaElement;
		};
		return media !== undefined && !media.paused && media.currentTime > 0;
	});

test("one Active Device plays and the other client follows and controls it", async ({
	browser,
	page: first,
	request,
}) => {
	test.setTimeout(120_000);
	await first.addInitScript(trackMediaElements);
	await first.goto("/settings");
	await first.getByRole("button", { name: "Import Music" }).click();
	const dialog = first.getByRole("dialog", { name: "Import Music" });
	await dialog.getByLabel("Audio files").setInputFiles({
		name: "long.flac",
		mimeType: "audio/flac",
		buffer: longFlac(),
	});
	await expect(dialog.getByText("1 of 1 ready", { exact: true })).toBeVisible({
		timeout: 30_000,
	});
	await dialog.getByRole("button", { name: "Confirm Import" }).click();
	await expect(dialog.getByText("Imported", { exact: true })).toBeVisible();
	await dialog.getByRole("button", { name: "Done" }).click();

	const secondContext = await browser.newContext();
	const second = await secondContext.newPage();
	await second.addInitScript(trackMediaElements);
	await second.goto("/library/albums");

	await first.goto("/library/tracks");
	await first.getByText(TITLE, { exact: true }).first().dblclick();
	await expect(control(first, "Pause")).toBeVisible();
	await expect.poll(() => isAudioPlaying(first)).toBe(true);
	await first.waitForTimeout(1500);

	// Following: same Track, named Active Device, and no sound of its own.
	await expect(second.getByText(/^Playing on /)).toBeVisible();
	await expect(second.getByText(TITLE).first()).toBeVisible();
	expect(await isAudioPlaying(second)).toBe(false);

	// Pausing elsewhere stops the Active Device.
	await control(second, "Pause").click();
	await expect(control(first, "Play")).toBeVisible();
	await expect.poll(() => isAudioPlaying(first)).toBe(false);
	const paused = await (await request.get("/api/v1/playback/state")).json();
	expect(paused.playing).toBe(false);
	expect(paused.positionMs).toBeGreaterThan(0);

	// Starting elsewhere moves the sound there, at the same playhead.
	await control(second, "Play").click();
	await expect.poll(() => isAudioPlaying(second)).toBe(true);
	await expect.poll(() => isAudioPlaying(first)).toBe(false);
	const moved = await (await request.get("/api/v1/playback/state")).json();
	expect(moved.positionMs).toBeGreaterThanOrEqual(paused.positionMs - 500);
	await expect(first.getByText(/^Playing on /)).toBeVisible();

	// A closed client leaves the state for the next one.
	await control(second, "Pause").click();
	await expect.poll(() => isAudioPlaying(second)).toBe(false);
	await expect(control(second, "Play")).toBeVisible();
	const left = await (await request.get("/api/v1/playback/state")).json();
	await secondContext.close();
	await first.reload();
	await expect(first.getByText(TITLE).first()).toBeVisible();
	await control(first, "Play").click();
	await expect.poll(() => isAudioPlaying(first)).toBe(true);
	const resumed = await (await request.get("/api/v1/playback/state")).json();
	expect(resumed.positionMs).toBeGreaterThanOrEqual(left.positionMs - 500);
	expect(resumed.playing).toBe(true);

	// Reloading the playing tab leaves nobody to obey: one press starts it.
	await first.reload();
	await expect(first.getByText(TITLE).first()).toBeVisible();
	await expect(control(first, "Play")).toBeVisible();
	await expect(first.getByText(/^Playing on /)).toBeHidden();
	await control(first, "Play").click();
	await expect.poll(() => isAudioPlaying(first)).toBe(true);
});
