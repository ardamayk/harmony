import { createHash } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
	type APIRequestContext,
	expect,
	type Locator,
	type Page,
	test,
} from "@playwright/test";
import {
	buildStrictMp3,
	type StrictTags,
} from "./fixtures/managed-import-audio.ts";

// Critical Web Managed Import journey (issue #54). The tests run serially
// against the real Music Server started by playwright.config.ts and share one
// unique run identifier so accumulated e2e state from earlier runs never
// collides with duplicate classification. The versioned HTTP contract is only
// used to verify the resulting library state, streams, and files on disk.

test.describe.configure({ mode: "serial" });

const RUN_ID = `e2e${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
const ARTIST = `Journey Artist ${RUN_ID}`;
const ALBUM = `Journey Album ${RUN_ID}`;
const ALPHA = `${RUN_ID} Alpha`;
const BETA = `${RUN_ID} Beta`;
const BETA_REMASTER = `${RUN_ID} Beta Remaster`;

// Paths must match the webServer env in playwright.config.ts, which the Go
// server resolves relative to its own working directory (../server).
const SERVER_DIR = fileURLToPath(new URL("../../server", import.meta.url));
const MANAGED_STORAGE_DIR = path.join(SERVER_DIR, "data/e2e-managed");
const FIXTURE_DIR = path.join(tmpdir(), `managed-import-${RUN_ID}`);

const STREAM_PATTERN = /\/api\/v1\/tracks\/[^/]+\/stream/;
const CANCEL_PROMPT = "Cancel this import and remove all uncommitted uploads?";

interface Fixture {
	path: string;
	bytes: Buffer;
	sha256: string;
}

function tags(title: string, track: string): StrictTags {
	return {
		title,
		artists: [ARTIST],
		albumArtist: ARTIST,
		album: ALBUM,
		track,
		genres: ["Electronic"],
	};
}

function writeFixture(
	name: string,
	bytes: Buffer,
	directory = FIXTURE_DIR,
): Fixture {
	mkdirSync(directory, { recursive: true });
	const filePath = path.join(directory, name);
	writeFileSync(filePath, bytes);
	return { path: filePath, bytes, sha256: sha256(bytes) };
}

function sha256(bytes: Buffer): string {
	return createHash("sha256").update(bytes).digest("hex");
}

const fixtures = {
	alpha: writeFixture("alpha.mp3", buildStrictMp3(tags(ALPHA, "1/3"))),
	beta: writeFixture("beta.mp3", buildStrictMp3(tags(BETA, "2/3"))),
	untitled: writeFixture(
		"untitled.mp3",
		buildStrictMp3(tags(`${RUN_ID} Untitled`, "3/3"), { omitFrames: ["TIT2"] }),
	),
	noArtwork: writeFixture(
		"no-artwork.mp3",
		buildStrictMp3(tags(`${RUN_ID} Artless`, "3/3"), { omitArtwork: true }),
	),
	betaRemaster: writeFixture(
		"beta-remaster.mp3",
		buildStrictMp3(tags(BETA_REMASTER, "2/3"), {
			userText: { E2E_VARIANT: "remaster" },
		}),
	),
	alphaReplacement: writeFixture(
		"alpha-replacement.mp3",
		buildStrictMp3(
			{ ...tags(ALPHA, "1/3"), genres: ["Electronic", "Ambient"] },
			{ userText: { E2E_VARIANT: "replacement" } },
		),
	),
	abandoned: writeFixture(
		"abandoned.mp3",
		buildStrictMp3(tags(`${RUN_ID} Abandoned`, "3/3"), {
			userText: { E2E_VARIANT: "abandoned" },
		}),
	),
};
// Exact Duplicate: identical bytes to alpha under a different client filename.
const alphaCopy = writeFixture("alpha-renamed-copy.mp3", fixtures.alpha.bytes);

interface TrackSummary {
	id: string;
	title: string;
	sizeBytes: number;
	albumId: string;
}

async function listRunTracks(
	request: APIRequestContext,
): Promise<TrackSummary[]> {
	const response = await request.get("/api/v1/library/tracks", {
		params: { q: RUN_ID, limit: 200 },
	});
	expect(response.ok()).toBe(true);
	const body = (await response.json()) as { items: TrackSummary[] };
	return body.items;
}

async function findTrack(
	request: APIRequestContext,
	title: string,
): Promise<TrackSummary> {
	const track = (await listRunTracks(request)).find(
		(item) => item.title === title,
	);
	expect(track, `Track ${title} should exist`).toBeDefined();
	return track as TrackSummary;
}

async function gotoTracks(page: Page): Promise<void> {
	await page.goto("/library/tracks");
	await expect(page.getByRole("heading", { name: "Tracks" })).toBeVisible({
		timeout: 15_000,
	});
}

function trackRow(page: Page, title: string): Locator {
	return page
		.getByRole("row")
		.filter({ has: page.getByText(title, { exact: true }) });
}

async function openImportDialog(page: Page): Promise<Locator> {
	await page.getByRole("button", { name: "Import Music" }).click();
	const dialog = page.getByRole("dialog", { name: "Import Music" });
	await expect(dialog).toBeVisible();
	return dialog;
}

function previewRow(dialog: Locator, text: string): Locator {
	return dialog.getByRole("article").filter({ hasText: text });
}

async function expectFocusInside(dialog: Locator): Promise<void> {
	// Base UI's focus guards wrap Tab around on the next animation frame.
	await expect
		.poll(
			() =>
				dialog.evaluate((element) => element.contains(document.activeElement)),
			{ message: "focus should stay inside the dialog" },
		)
		.toBe(true);
}

function stagingFiles(): string[] {
	const staging = path.join(MANAGED_STORAGE_DIR, ".staging");
	return existsSync(staging) ? readdirSync(staging) : [];
}

function managedFiles(directory = MANAGED_STORAGE_DIR): string[] {
	if (!existsSync(directory)) return [];
	return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
		const entryPath = path.join(directory, entry.name);
		return entry.isDirectory() ? managedFiles(entryPath) : [entryPath];
	});
}

function managedFileHashes(): Set<string> {
	return new Set(managedFiles().map((file) => sha256(readFileSync(file))));
}

// Staging files abandoned by earlier (possibly failed) runs are reaped by the
// server's inactivity cleanup, so assertions only track files this run adds.
function newStagingFiles(before: Set<string>): string[] {
	return stagingFiles().filter((name) => !before.has(name));
}

test("Tracks plus action imports a mixed valid and invalid batch through preview and confirmation", async ({
	page,
	request,
}) => {
	const stagingBefore = new Set(stagingFiles());
	await gotoTracks(page);
	await expect(trackRow(page, ALPHA)).toHaveCount(0);

	// Slow the first upload down so the live region and progress bar are
	// observable, then let the real request through.
	let delayed = false;
	await page.route("**/api/v1/imports/*/file", async (route) => {
		if (!delayed) {
			delayed = true;
			await new Promise((resolve) => setTimeout(resolve, 800));
		}
		await route.continue();
	});

	const dialog = await openImportDialog(page);
	await expect(
		dialog.getByText(
			"Upload audio files, review each result, then confirm selected Tracks.",
		),
	).toBeVisible();
	await dialog
		.getByLabel("Audio files")
		.setInputFiles([
			fixtures.alpha.path,
			fixtures.beta.path,
			fixtures.untitled.path,
			fixtures.noArtwork.path,
		]);

	const liveRegion = dialog.locator("[aria-live='polite']");
	await expect(liveRegion).toContainText("of 4 ready");
	await expect(
		dialog.getByRole("progressbar", { name: "alpha.mp3 upload progress" }),
	).toBeVisible();
	// Close stays available once the batch exists (cancelling asks for
	// confirmation instead), so the lock is not asserted here.

	await expect(
		dialog.getByRole("heading", { name: "Import Preview" }),
	).toBeVisible();
	await expect(liveRegion).toHaveText("3 of 4 ready", { timeout: 20_000 });
	await page.unroute("**/api/v1/imports/*/file");

	const alphaRow = previewRow(dialog, ALPHA);
	const betaRow = previewRow(dialog, BETA);
	const untitledRow = previewRow(dialog, "untitled.mp3");
	const artlessRow = previewRow(dialog, `${RUN_ID} Artless`);
	await expect(alphaRow.getByText("Accepted")).toBeVisible();
	await expect(betaRow.getByText("Accepted")).toBeVisible();
	await expect(untitledRow.getByText("Rejected")).toBeVisible();
	await expect(artlessRow.getByText("Accepted")).toBeVisible();
	await artlessRow.getByRole("checkbox").uncheck();
	await expect(
		untitledRow.getByRole("list", { name: "Import errors" }),
	).not.toBeEmpty();
	await expect(untitledRow.getByRole("checkbox")).toBeDisabled();
	await expect(
		alphaRow.getByRole("checkbox", { name: "Select alpha.mp3" }),
	).toBeChecked();
	await expect(
		betaRow.getByRole("checkbox", { name: "Select beta.mp3" }),
	).toBeChecked();

	// A failed confirmation surfaces an assertive error and keeps the batch
	// open for a retry.
	let injected = false;
	await page.route("**/api/v1/import-batches/*/confirm", async (route) => {
		if (injected) {
			await route.continue();
			return;
		}
		injected = true;
		await route.fulfill({
			status: 500,
			contentType: "application/json",
			body: JSON.stringify({
				code: "internal",
				message: "Injected confirmation failure.",
			}),
		});
	});
	const confirmButton = dialog.getByRole("button", { name: "Confirm Import" });
	await expect(confirmButton).toBeEnabled();
	await confirmButton.click();
	await expect(dialog.getByRole("alert")).toHaveText(
		"Injected confirmation failure.",
	);

	await expect(confirmButton).toBeEnabled();
	await confirmButton.click();
	await expect(alphaRow.getByText("Imported")).toBeVisible({ timeout: 20_000 });
	await expect(betaRow.getByText("Imported")).toBeVisible();
	await expect(untitledRow.getByText("Rejected")).toBeVisible();
	await expect(confirmButton).toBeHidden();
	await dialog.getByRole("button", { name: "Done" }).click();
	await expect(dialog).toBeHidden();

	await expect(trackRow(page, ALPHA)).toBeVisible();
	await expect(trackRow(page, BETA)).toBeVisible();
	await expect(page.getByRole("row").filter({ hasText: RUN_ID })).toHaveCount(
		2,
	);

	const tracks = await listRunTracks(request);
	expect(tracks.map((track) => track.title).sort()).toEqual([ALPHA, BETA]);
	await expect.poll(() => newStagingFiles(stagingBefore)).toEqual([]);
});

test("Import Preview skips identical bytes and separates a conflicting Album edition", async ({
	page,
	request,
}) => {
	await gotoTracks(page);
	const dialog = await openImportDialog(page);
	await dialog
		.getByLabel("Audio files")
		.setInputFiles([alphaCopy.path, fixtures.betaRemaster.path]);

	const copyRow = previewRow(dialog, "Exact Duplicate");
	await expect(copyRow).toBeVisible({ timeout: 20_000 });
	await expect(
		copyRow.getByText(`File bytes already belong to ${ALPHA}.`),
	).toBeVisible();
	await expect(copyRow.getByText("Rejected")).toBeVisible();
	await expect(copyRow.getByRole("checkbox")).toHaveCount(0);
	await copyRow.getByText("View existing Track").click();
	await expect(copyRow.locator("dd", { hasText: ARTIST })).toBeVisible();
	await expect(copyRow.locator("dd", { hasText: ALBUM })).toBeVisible();

	const remasterRow = previewRow(dialog, BETA_REMASTER);
	await expect(
		remasterRow.getByText(
			"Another title occupies this Album position. Skip this file or create a separate Album.",
		),
	).toBeVisible();
	await dialog.getByLabel(`Album destination for ${ALBUM}`).click();
	await page
		.getByRole("option", { name: "Create separate album", exact: true })
		.click();
	await expect(
		remasterRow.getByText(
			"Another title occupies this Album position. Skip this file or create a separate Album.",
		),
	).toBeHidden();
	await remasterRow.getByRole("checkbox").check();
	await dialog.getByRole("button", { name: "Confirm Import" }).click();

	await expect(remasterRow.getByText("Imported")).toBeVisible({
		timeout: 20_000,
	});
	await expect(copyRow.getByText("Rejected")).toBeVisible();
	await dialog.getByRole("button", { name: "Done" }).click();

	await expect(trackRow(page, BETA_REMASTER)).toBeVisible();
	const tracks = await listRunTracks(request);
	expect(tracks.map((track) => track.title).sort()).toEqual([
		ALPHA,
		BETA,
		BETA_REMASTER,
	]);
});

test("explicit Track Replacement keeps the Track identity and swaps the managed bytes", async ({
	page,
	request,
}) => {
	const before = await findTrack(request, ALPHA);
	await gotoTracks(page);

	await trackRow(page, ALPHA).click({ button: "right" });
	await page.getByRole("menuitem", { name: "Replace file" }).click();
	const dialog = page.getByRole("dialog", { name: `Replace ${ALPHA}` });
	await expect(dialog).toBeVisible();
	await expect(
		dialog.getByText(
			"The Track keeps its identity, Playlist and Queue references. The previous managed file is deleted only after the replacement is verified.",
		),
	).toBeVisible();
	await expect(
		dialog.getByRole("button", { name: "Replace track" }),
	).toBeDisabled();

	await dialog
		.getByLabel("Replacement audio file")
		.setInputFiles(fixtures.alphaReplacement.path);
	const review = dialog.locator(
		"section[aria-label='Track Replacement review']",
	);
	await expect(review).toBeVisible({ timeout: 20_000 });
	await expect(review.getByText("Old file deletion")).toBeVisible();
	await expect(
		review.getByText(
			/will be deleted permanently after the replacement is verified/,
		),
	).toBeVisible();
	await expect(review.getByText("Playlists kept")).toBeVisible();

	await dialog.getByRole("button", { name: "Replace track" }).click();
	await expect(dialog.getByRole("status")).toHaveText(
		`${ALPHA} was replaced. Playback of this Track was stopped if it was active.`,
		{ timeout: 20_000 },
	);
	await dialog.getByRole("button", { name: "Done" }).click();
	await expect(dialog).toBeHidden();

	const after = await findTrack(request, ALPHA);
	expect(after.id).toBe(before.id);
	expect(after.sizeBytes).toBe(fixtures.alphaReplacement.bytes.length);
	expect(after.sizeBytes).not.toBe(before.sizeBytes);
});

test("committed Tracks appear in library views and stream bit-for-bit", async ({
	page,
	request,
}) => {
	const expected: Record<string, Fixture> = {
		[ALPHA]: fixtures.alphaReplacement,
		[BETA]: fixtures.beta,
		[BETA_REMASTER]: fixtures.betaRemaster,
	};
	for (const [title, fixture] of Object.entries(expected)) {
		const track = await findTrack(request, title);
		const response = await request.get(`/api/v1/tracks/${track.id}/stream`);
		expect(response.status(), `${title} stream status`).toBe(200);
		expect(sha256(await response.body()), `${title} stream bytes`).toBe(
			fixture.sha256,
		);
		const cover = await request.get(
			`/api/v1/library/albums/${track.albumId}/cover`,
		);
		expect(cover.ok(), `${title} album cover`).toBe(true);
	}

	await page.goto("/library/albums");
	await expect(
		page.getByRole("link").filter({ hasText: ALBUM }).first(),
	).toBeVisible({
		timeout: 15_000,
	});

	await gotoTracks(page);
	const streamResponse = page.waitForResponse(
		(response) =>
			STREAM_PATTERN.test(response.url()) &&
			response.request().method() === "GET",
	);
	await trackRow(page, BETA).click();
	expect([200, 206]).toContain((await streamResponse).status());
	await expect(page.getByText("Nothing playing")).toHaveCount(0);
});

test("Permanent Track Deletion requires an explicit destructive confirmation", async ({
	page,
	request,
}) => {
	const target = await findTrack(request, BETA_REMASTER);
	await gotoTracks(page);

	await trackRow(page, BETA_REMASTER).click({ button: "right" });
	await page.getByRole("menuitem", { name: "Delete track" }).click();
	const dialog = page.getByRole("dialog", {
		name: `Permanently delete ${BETA_REMASTER}?`,
	});
	await expect(dialog).toBeVisible();
	await expect(
		dialog.getByText(
			"This cannot be undone. No trash or restore copy will be kept.",
		),
	).toBeVisible();
	await expect(dialog.getByText("Managed file")).toBeVisible();
	await expect(dialog.getByText("File size")).toBeVisible();
	await expectFocusInside(dialog);

	// Cancelling leaves the library untouched.
	await dialog.getByRole("button", { name: "Cancel" }).click();
	await expect(dialog).toBeHidden();
	await expect(trackRow(page, BETA_REMASTER)).toBeVisible();
	expect(
		(await request.get(`/api/v1/tracks/${target.id}/stream`)).status(),
	).toBe(200);

	await trackRow(page, BETA_REMASTER).click({ button: "right" });
	await page.getByRole("menuitem", { name: "Delete track" }).click();
	const deleteButton = dialog.getByRole("button", {
		name: "Delete permanently",
	});
	await expect(deleteButton).toBeEnabled();
	await deleteButton.click();
	await expect(dialog).toBeHidden();
	await expect(trackRow(page, BETA_REMASTER)).toHaveCount(0);
	await expect(trackRow(page, BETA)).toBeVisible();

	expect(
		(await request.get(`/api/v1/tracks/${target.id}/stream`)).status(),
	).toBe(404);
	const remaining = await listRunTracks(request);
	expect(remaining.map((track) => track.title).sort()).toEqual([ALPHA, BETA]);
});

test("Import Music dialog is keyboard accessible and cancelling cleans staging", async ({
	page,
	request,
}) => {
	const before = await listRunTracks(request);
	const stagingBefore = new Set(stagingFiles());
	await gotoTracks(page);

	// Keyboard open, focus trap, Escape close with focus restoration.
	const importButton = page.getByRole("button", { name: "Import Music" });
	await importButton.focus();
	await page.keyboard.press("Enter");
	const dialog = page.getByRole("dialog", { name: "Import Music" });
	await expect(dialog).toBeVisible();
	await expectFocusInside(dialog);
	for (let step = 0; step < 12; step++) {
		await page.keyboard.press("Tab");
		await expectFocusInside(dialog);
	}
	await page.keyboard.press("Escape");
	await expect(dialog).toBeHidden();
	await expect(importButton).toBeFocused();

	// An uncommitted batch warns before closing; declining keeps it open.
	await importButton.click();
	await dialog.getByLabel("Audio files").setInputFiles(fixtures.abandoned.path);
	await expect(
		previewRow(dialog, `${RUN_ID} Abandoned`).getByText("Accepted"),
	).toBeVisible({
		timeout: 20_000,
	});
	const prompts: string[] = [];
	page.once("dialog", (prompt) => {
		prompts.push(prompt.message());
		void prompt.dismiss();
	});
	await page.keyboard.press("Escape");
	await expect(dialog).toBeVisible();
	expect(prompts).toEqual([CANCEL_PROMPT]);
	expect(newStagingFiles(stagingBefore).length).toBeGreaterThan(0);

	// Accepting the warning cancels the batch and removes staging.
	page.once("dialog", (prompt) => {
		prompts.push(prompt.message());
		void prompt.accept();
	});
	const cancelled = page.waitForResponse(
		(response) =>
			/\/api\/v1\/import-batches\/[^/]+$/.test(response.url()) &&
			response.request().method() === "DELETE",
	);
	await dialog.getByRole("button", { name: "Cancel" }).click();
	expect((await cancelled).status()).toBe(204);
	await expect(dialog).toBeHidden();
	expect(prompts).toEqual([CANCEL_PROMPT, CANCEL_PROMPT]);
	await expect.poll(() => newStagingFiles(stagingBefore)).toEqual([]);

	await expect(trackRow(page, `${RUN_ID} Abandoned`)).toHaveCount(0);
	const after = await listRunTracks(request);
	expect(after.map((track) => track.title).sort()).toEqual(
		before.map((track) => track.title).sort(),
	);
});

test("Album deletion previews and permanently deletes every remaining Track", async ({
	page,
	request,
}) => {
	const targets = await listRunTracks(request);
	expect(targets.map((track) => track.title).sort()).toEqual([ALPHA, BETA]);
	const targetHashes = [fixtures.alphaReplacement.sha256, fixtures.beta.sha256];
	for (const hash of targetHashes) {
		expect(managedFileHashes(), `managed file ${hash} should exist`).toContain(
			hash,
		);
	}

	await page.goto("/library/albums");
	const albumCard = page.getByRole("link").filter({ hasText: ALBUM }).first();
	await expect(albumCard).toBeVisible({ timeout: 15_000 });
	await albumCard.click({ button: "right" });
	await page.getByRole("menuitem", { name: "Delete album" }).click();

	const dialog = page.getByRole("dialog", {
		name: `Permanently delete every track of ${ALBUM}?`,
	});
	await expect(dialog).toBeVisible();
	await expect(dialog.getByText("2 tracks")).toBeVisible();
	const deleteButton = dialog.getByRole("button", {
		name: "Delete permanently",
	});
	await expect(deleteButton).toBeEnabled();
	const deletionResponse = page.waitForResponse(
		(response) =>
			/\/api\/v1\/library\/albums\/[^/]+$/.test(response.url()) &&
			response.request().method() === "DELETE",
	);
	await deleteButton.click();
	const deletionResult = await deletionResponse;
	expect(deletionResult.status()).toBe(200);
	expect(await deletionResult.json()).toMatchObject({ stoppedAt: null });

	await expect(dialog).toBeHidden();
	await expect(page.getByText("All tracks deleted")).toBeVisible();
	await expect(albumCard).toHaveCount(0);
	await expect.poll(() => listRunTracks(request)).toEqual([]);
	for (const target of targets) {
		expect(
			(await request.get(`/api/v1/tracks/${target.id}/stream`)).status(),
		).toBe(404);
	}
	for (const hash of targetHashes) {
		expect(
			managedFileHashes(),
			`managed file ${hash} should be deleted`,
		).not.toContain(hash);
	}
});
