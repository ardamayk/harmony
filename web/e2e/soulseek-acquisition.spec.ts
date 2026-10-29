import { expect, type Page, test } from "@playwright/test";

// Soulseek Acquisition journey in the Web Client. The real Music Server runs
// without slskd, so the capability and every Soulseek route are stubbed at
// the network layer; the journey checks what the listener sees and which
// requests the client sends, not the server's acquisition worker.

const SEARCH_ID = "00000000-0000-4000-8000-00000000a001";
const ACQUISITION_ID = "00000000-0000-4000-8000-00000000a002";
const BATCH_ID = "00000000-0000-4000-8000-00000000a003";
const JOB_ID = "00000000-0000-4000-8000-00000000a004";
const QUERY = "Daft Punk One More Time";

const externalTrack = {
	id: "e2e-external.signature",
	title: "One More Time (Official Video)",
	artistName: "Daft Punk",
	durationMs: 320_000,
	webpageUrl: null,
	extractor: "Youtube",
	hasArtwork: false,
};

const flacQuality = {
	format: "flac",
	lossless: true,
	bitDepth: 24,
	sampleRateHz: 48_000,
};

const versions = [
	{
		versionKey: "flac-24-48000",
		quality: flacQuality,
		fileCount: 2,
		totalBytes: 80 * 1024 * 1024,
		sourceCount: 2,
		completeSourceCount: 1,
		sources: [
			{
				sourceId: "source-a",
				username: "alice",
				folder: "Music\\Daft Punk\\Discovery",
				fileCount: 2,
				totalBytes: 80 * 1024 * 1024,
				complete: true,
				hasFreeUploadSlot: true,
				queueLength: 0,
				uploadSpeedBytesPerSecond: 2 * 1024 * 1024,
			},
			{
				sourceId: "source-b",
				username: "bob",
				folder: "share\\Discovery",
				fileCount: 1,
				totalBytes: 40 * 1024 * 1024,
				complete: false,
				hasFreeUploadSlot: false,
				queueLength: 3,
				uploadSpeedBytesPerSecond: 512 * 1024,
			},
		],
	},
	{
		versionKey: "mp3-320-cbr",
		quality: {
			format: "mp3",
			lossless: false,
			bitrateKbps: 320,
			variableBitrate: false,
		},
		fileCount: 2,
		totalBytes: 20 * 1024 * 1024,
		sourceCount: 1,
		completeSourceCount: 1,
		sources: [
			{
				sourceId: "source-c",
				username: "carol",
				folder: "mp3\\Discovery",
				fileCount: 2,
				totalBytes: 20 * 1024 * 1024,
				complete: true,
				hasFreeUploadSlot: true,
				queueLength: 0,
				uploadSpeedBytesPerSecond: 1024 * 1024,
			},
		],
	},
];

type AcquisitionState = "downloading" | "review" | "cancelled";

function acquisition(state: AcquisitionState) {
	return {
		acquisitionId: ACQUISITION_ID,
		query: QUERY,
		scope: "album",
		versionKey: "flac-24-48000",
		quality: flacQuality,
		state,
		...(state === "review" ? { batchId: BATCH_ID } : {}),
		currentSource: {
			sourceId: "source-a",
			username: "alice",
			folder: "Music\\Daft Punk\\Discovery",
		},
		sourcesTried: 1,
		sourceCount: 2,
		files: [
			{
				filename: "01 One More Time.flac",
				state: state === "downloading" ? "downloading" : "staged",
				bytesTransferred:
					state === "downloading" ? 20 * 1024 * 1024 : 40 * 1024 * 1024,
				totalBytes: 40 * 1024 * 1024,
				...(state === "downloading" ? {} : { jobId: JOB_ID }),
			},
		],
		createdAt: "2026-10-29T10:00:00Z",
		updatedAt: "2026-10-29T10:01:00Z",
	};
}

const reviewBatch = {
	id: BATCH_ID,
	status: "uploading",
	revision: 2,
	files: [
		{
			jobId: JOB_ID,
			state: "accepted",
			status: "awaiting_confirmation",
			revision: 2,
			validationProgress: 100,
			originalFilename: "01 One More Time.flac",
			selected: true,
			phase: "ready",
			preview: {
				jobId: JOB_ID,
				status: "awaiting_confirmation",
				revision: 2,
				duplicateClassification: "none",
				file: {
					originalFilename: "01 One More Time.flac",
					contentSha256: "a".repeat(64),
					sizeBytes: 40 * 1024 * 1024,
					format: "flac",
					container: "flac",
					codec: "flac",
					durationMs: 320_000,
					sampleRateHz: 48_000,
					bitDepth: 24,
					channelCount: 2,
					bitrateKbps: 1_000,
					artworkMediaType: "",
					genres: ["Electronic"],
					title: "One More Time",
					artists: ["Daft Punk"],
					albumArtists: ["Daft Punk"],
					album: "Discovery",
					albumKey: "daft-punk-discovery",
					discNo: 1,
					trackNo: 1,
				},
			},
		},
	],
};

type Stub = {
	state: AcquisitionState;
	acquisitionStarts: unknown[];
	cancels: number;
	batchCancels: number;
};

async function stubSoulseek(page: Page): Promise<Stub> {
	const stub: Stub = {
		state: "downloading",
		acquisitionStarts: [],
		cancels: 0,
		batchCancels: 0,
	};
	await page.route("**/api/v1/health", async (route) => {
		const response = await route.fetch();
		const health = await response.json();
		await route.fulfill({
			response,
			json: {
				...health,
				capabilities: [
					...health.capabilities,
					"external-source.v1",
					"soulseek-acquisition.v1",
				],
				externalSource: { name: "YouTube" },
			},
		});
	});
	await page.route("**/api/v1/external/search?*", (route) =>
		route.fulfill({ json: { source: "ytsearch", items: [externalTrack] } }),
	);
	await page.route("**/api/v1/soulseek/status", (route) =>
		route.fulfill({
			json: {
				slskd: { reachable: true, version: "0.22.5" },
				soulseek: { connected: true, loggedIn: true, username: "e2e-operator" },
				sharing: { state: "not_shared" },
			},
		}),
	);
	await page.route("**/api/v1/soulseek/searches", (route) =>
		route.fulfill({ status: 201, json: { searchId: SEARCH_ID } }),
	);
	await page.route(`**/api/v1/soulseek/searches/${SEARCH_ID}`, (route) =>
		route.fulfill({
			json: {
				searchId: SEARCH_ID,
				query: QUERY,
				scope: "album",
				state: "complete",
				versions,
			},
		}),
	);
	await page.route("**/api/v1/soulseek/acquisitions", async (route) => {
		if (route.request().method() === "POST") {
			stub.acquisitionStarts.push(route.request().postDataJSON());
			await route.fulfill({ status: 201, json: acquisition(stub.state) });
			return;
		}
		await route.fulfill({
			json: { items: [acquisition(stub.state)] },
		});
	});
	await page.route(
		`**/api/v1/soulseek/acquisitions/${ACQUISITION_ID}`,
		async (route) => {
			if (route.request().method() === "DELETE") {
				stub.cancels += 1;
				stub.state = "cancelled";
				await route.fulfill({ status: 204 });
				return;
			}
			await route.fulfill({ json: acquisition(stub.state) });
		},
	);
	await page.route(`**/api/v1/import-batches/${BATCH_ID}`, async (route) => {
		if (route.request().method() === "DELETE") {
			stub.batchCancels += 1;
			await route.fulfill({ status: 204 });
			return;
		}
		await route.fulfill({ json: reviewBatch });
	});
	return stub;
}

// The health stub passes the real response through; a refetch may still be
// in flight when a test ends.
test.afterEach(async ({ page }) => {
	await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("installs an External Track from Soulseek and reviews it in Import Music", async ({
	page,
}) => {
	const stub = await stubSoulseek(page);
	await page.goto("/library/tracks");
	await expect(page.getByRole("heading", { name: "Tracks" })).toBeVisible({
		timeout: 15_000,
	});

	// Search: the External row offers Install.
	await page.getByRole("button", { name: /^Search your library/ }).click();
	const search = page.getByTestId("library-search-dialog");
	await search.getByRole("combobox").fill("one more time");
	await search
		.getByRole("button", { name: "Install One More Time (Official Video)" })
		.click();

	// Install dialog: Album by default, the query without the video noise.
	const install = page.getByRole("dialog", { name: "Install" });
	await expect(install).toBeVisible();
	await expect(install.getByLabel("Soulseek query")).toHaveValue(QUERY);
	await expect(install.getByRole("radio", { name: "Album" })).toBeChecked();
	await install.getByRole("button", { name: "Search Soulseek" }).click();

	const flac = install.getByRole("listitem", { name: "FLAC 24-bit / 48 kHz" });
	await expect(flac).toContainText(
		"2 files · 80.0 MiB · 2 sources (1 complete)",
	);
	await expect(
		install.getByRole("listitem", { name: "MP3 320 kbps" }),
	).toBeVisible();
	await flac
		.getByRole("button", { name: "Sources for FLAC 24-bit / 48 kHz" })
		.click();
	await expect(flac.getByText("1/2 files")).toBeVisible();
	await expect(install).not.toContainText(/best/i);

	await flac
		.getByRole("button", { name: "Install FLAC 24-bit / 48 kHz" })
		.click();
	await expect
		.poll(() => stub.acquisitionStarts)
		.toEqual([{ searchId: SEARCH_ID, versionKey: "flac-24-48000" }]);

	// Import Music follows the download.
	const importDialog = page.getByRole("dialog", { name: "Import Music" });
	await expect(importDialog).toBeVisible();
	const row = importDialog.getByRole("listitem", { name: QUERY });
	await expect(row).toContainText("Downloading from alice · sources tried 1/2");
	await expect(
		row.getByRole("progressbar", {
			name: "01 One More Time.flac download progress",
		}),
	).toHaveAttribute("aria-valuenow", "50");

	// The worker finishes; the list polls into the review.
	stub.state = "review";
	await row.getByRole("button", { name: "Review import" }).click({
		timeout: 10_000,
	});
	const preview = importDialog.getByRole("region", { name: "Import Preview" });
	await expect(
		preview.getByText("One More Time", { exact: true }),
	).toBeVisible();
	await expect(
		importDialog.getByRole("button", { name: "Confirm Import" }),
	).toBeEnabled();

	// Closing the dialog never cancels the acquisition.
	await importDialog
		.getByRole("button", { name: "Close Import Music" })
		.click();
	await expect(importDialog).toBeHidden();
	expect(stub.cancels).toBe(0);
	expect(stub.batchCancels).toBe(0);

	// Cancelling asks first, then cancels the acquisition itself.
	await page.getByRole("button", { name: "Import Music" }).click();
	await expect(importDialog).toBeVisible();
	page.once("dialog", (dialog) => {
		expect(dialog.message()).toBe(
			"Cancel this acquisition and delete its downloaded files?",
		);
		void dialog.accept();
	});
	await importDialog.getByRole("button", { name: `Cancel ${QUERY}` }).click();
	await expect(importDialog.getByText("From Soulseek")).toBeHidden();
	expect(stub.cancels).toBe(1);
	expect(stub.batchCancels).toBe(0);
});

test("shows the Soulseek state and the operator's responsibility in settings", async ({
	page,
}) => {
	await stubSoulseek(page);
	await page.goto("/settings");
	const section = page.getByRole("region", { name: "Soulseek" });
	await expect(section).toContainText("Reachable · version 0.22.5");
	await expect(section).toContainText("Logged in as e2e-operator");
	await expect(section).toContainText("Your library is not shared.");
	await expect(section).toContainText(
		"The operator shares Managed Storage in slskd’s configuration.",
	);
	await expect(section).toContainText(
		"Downloading or sharing copyrighted music without permission is unlawful in many places.",
	);
});
