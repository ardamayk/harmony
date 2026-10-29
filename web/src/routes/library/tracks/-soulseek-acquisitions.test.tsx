import {
	ApiError,
	type ManagedImportBatch,
	type SoulseekAcquisition,
} from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
	within,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { ImportMusicDialog } from "./-import-music-dialog";

const mocks = vi.hoisted(() => ({
	hasSoulseek: true,
	listSoulseekAcquisitions: vi.fn(),
	cancelSoulseekAcquisition: vi.fn(),
	getManagedImportBatch: vi.fn(),
	heartbeatManagedImportBatch: vi.fn(),
	cancelManagedImportBatch: vi.fn(),
	confirmManagedImportBatch: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		listSoulseekAcquisitions: mocks.listSoulseekAcquisitions,
		cancelSoulseekAcquisition: mocks.cancelSoulseekAcquisition,
		getManagedImportBatch: mocks.getManagedImportBatch,
		heartbeatManagedImportBatch: mocks.heartbeatManagedImportBatch,
		cancelManagedImportBatch: mocks.cancelManagedImportBatch,
		confirmManagedImportBatch: mocks.confirmManagedImportBatch,
	},
}));
vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: (capability: string) =>
		capability === "soulseek-acquisition.v1" && mocks.hasSoulseek,
}));

const BATCH_ID = "00000000-0000-4000-8000-00000000b001";

function acquisition(
	overrides: Partial<SoulseekAcquisition> = {},
): SoulseekAcquisition {
	return {
		acquisitionId: "acq-1",
		query: "Daft Punk Discovery",
		scope: "album",
		versionKey: "flac-24-48000",
		quality: {
			format: "flac",
			lossless: true,
			bitDepth: 24,
			sampleRateHz: 48_000,
		},
		state: "downloading",
		currentSource: { sourceId: "s2", username: "alice", folder: "Discovery" },
		sourcesTried: 2,
		sourceCount: 7,
		files: [
			{
				filename: "01 One More Time.flac",
				state: "downloading",
				bytesTransferred: 25,
				totalBytes: 100,
			},
			{
				filename: "02 Aerodynamic.flac",
				state: "queued",
				bytesTransferred: 0,
				totalBytes: 100,
			},
		],
		createdAt: "2026-10-29T10:00:00Z",
		updatedAt: "2026-10-29T10:01:00Z",
		...overrides,
	};
}

const reviewBatch: ManagedImportBatch = {
	id: BATCH_ID,
	status: "uploading",
	revision: 3,
	files: [
		{
			jobId: "job-1",
			state: "accepted",
			status: "awaiting_confirmation",
			revision: 2,
			validationProgress: 100,
			selected: true,
			originalFilename: "01 One More Time.flac",
			preview: {
				jobId: "job-1",
				status: "awaiting_confirmation",
				revision: 2,
				file: {
					originalFilename: "01 One More Time.flac",
					albumKey: "album-key",
					contentSha256: "abc",
					format: "flac",
					discNo: 1,
					trackNo: 1,
					albumArtists: ["Daft Punk"],
					title: "One More Time",
					artists: ["Daft Punk"],
					album: "Discovery",
				},
			} as never,
		},
	],
};

function renderImportDialog(onOpenChange = vi.fn()) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	render(
		<QueryClientProvider client={client}>
			<ImportMusicDialog
				isOpen
				onOpenChange={onOpenChange}
				onCommitted={() => Promise.resolve()}
			/>
		</QueryClientProvider>,
	);
	return { onOpenChange };
}

describe("Soulseek Acquisitions in Import Music", () => {
	beforeEach(() => {
		vi.spyOn(window, "confirm").mockReturnValue(true);
		mocks.cancelSoulseekAcquisition.mockResolvedValue(undefined);
		mocks.heartbeatManagedImportBatch.mockResolvedValue(undefined);
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
		vi.restoreAllMocks();
		mocks.hasSoulseek = true;
	});

	it("stays hidden without the capability", async () => {
		mocks.hasSoulseek = false;
		mocks.listSoulseekAcquisitions.mockResolvedValue({
			items: [acquisition()],
		});
		renderImportDialog();

		await screen.findByRole("dialog", { name: "Import Music" });
		expect(screen.queryByText("From Soulseek")).toBeNull();
		expect(mocks.listSoulseekAcquisitions).not.toHaveBeenCalled();
	});

	it("shows a download with per-file progress, its source and sources tried", async () => {
		mocks.listSoulseekAcquisitions.mockResolvedValue({
			items: [acquisition()],
		});
		renderImportDialog();

		const row = await screen.findByRole("listitem", {
			name: "Daft Punk Discovery",
		});
		expect(row.textContent).toContain("Album · FLAC 24-bit / 48 kHz");
		expect(row.textContent).toContain(
			"Downloading from alice · sources tried 2/7",
		);
		const progress = within(row).getByRole("progressbar", {
			name: "01 One More Time.flac download progress",
		});
		expect(progress.getAttribute("aria-valuenow")).toBe("25");
		expect(row.textContent).toContain("25%");
		expect(row.textContent).toContain("Queued");
	});

	it("shows why an acquisition failed and hides finished ones", async () => {
		mocks.listSoulseekAcquisitions.mockResolvedValue({
			items: [
				acquisition({
					state: "failed",
					failureReason: "No source of this version is left.",
				}),
				acquisition({
					acquisitionId: "acq-2",
					query: "Done",
					state: "completed",
				}),
				acquisition({
					acquisitionId: "acq-3",
					query: "Gone",
					state: "cancelled",
				}),
			],
		});
		renderImportDialog();

		const row = await screen.findByRole("listitem", {
			name: "Daft Punk Discovery",
		});
		expect(row.textContent).toContain(
			"Failed: No source of this version is left.",
		);
		expect(within(row).queryByRole("button", { name: /Cancel/ })).toBeNull();
		// Nothing arrived, so there is nothing to review.
		expect(
			within(row).queryByRole("button", { name: "Review import" }),
		).toBeNull();
		expect(screen.queryByText("Done")).toBeNull();
		expect(screen.queryByText("Gone")).toBeNull();
	});

	it("dismisses a failed acquisition without confirmation", async () => {
		const failed = acquisition({
			state: "failed",
			failureReason: "No source of this version is left.",
		});
		mocks.listSoulseekAcquisitions
			.mockResolvedValueOnce({ items: [failed] })
			.mockResolvedValue({ items: [] });
		renderImportDialog();

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Dismiss Daft Punk Discovery",
			}),
		);

		expect(window.confirm).not.toHaveBeenCalled();
		await waitFor(() =>
			expect(mocks.cancelSoulseekAcquisition).toHaveBeenCalledWith("acq-1"),
		);
		await waitFor(() => expect(screen.queryByText("From Soulseek")).toBeNull());
	});

	it("removes a dismissed acquisition before the list refreshes", async () => {
		const failed = acquisition({ state: "failed" });
		mocks.listSoulseekAcquisitions
			.mockResolvedValueOnce({ items: [failed] })
			.mockReturnValue(new Promise(() => {}));
		renderImportDialog();

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Dismiss Daft Punk Discovery",
			}),
		);

		await waitFor(() => expect(screen.queryByText("From Soulseek")).toBeNull());
	});

	it("cancels a download after confirmation", async () => {
		mocks.listSoulseekAcquisitions
			.mockResolvedValueOnce({ items: [acquisition()] })
			.mockResolvedValue({ items: [acquisition({ state: "cancelled" })] });
		renderImportDialog();

		fireEvent.click(
			await screen.findByRole("button", { name: "Cancel Daft Punk Discovery" }),
		);

		expect(window.confirm).toHaveBeenCalledWith(
			"Cancel this acquisition and delete its downloaded files?",
		);
		await waitFor(() =>
			expect(mocks.cancelSoulseekAcquisition).toHaveBeenCalledWith("acq-1"),
		);
		await waitFor(() => expect(screen.queryByText("From Soulseek")).toBeNull());
	});

	it("keeps a download when the listener declines the confirmation", async () => {
		vi.mocked(window.confirm).mockReturnValue(false);
		mocks.listSoulseekAcquisitions.mockResolvedValue({
			items: [acquisition()],
		});
		renderImportDialog();

		fireEvent.click(
			await screen.findByRole("button", { name: "Cancel Daft Punk Discovery" }),
		);
		expect(mocks.cancelSoulseekAcquisition).not.toHaveBeenCalled();
	});

	it("says a committing import can no longer be cancelled", async () => {
		mocks.listSoulseekAcquisitions.mockResolvedValue({
			items: [acquisition({ state: "importing" })],
		});
		mocks.cancelSoulseekAcquisition.mockRejectedValue(
			new ApiError(409, {
				error: "conflict",
				code: "acquisition_state_conflict",
				message: "committing",
			}),
		);
		renderImportDialog();

		fireEvent.click(
			await screen.findByRole("button", { name: "Cancel Daft Punk Discovery" }),
		);
		expect((await screen.findByRole("alert")).textContent).toBe(
			"This import is being committed and can no longer be cancelled.",
		);
	});

	describe("review", () => {
		beforeEach(() => {
			mocks.listSoulseekAcquisitions.mockResolvedValue({
				items: [acquisition({ state: "review", batchId: BATCH_ID })],
			});
			mocks.getManagedImportBatch.mockResolvedValue(reviewBatch);
		});

		it("opens the acquisition's Import Preview", async () => {
			renderImportDialog();
			fireEvent.click(
				await screen.findByRole("button", { name: "Review import" }),
			);

			const preview = await screen.findByRole("region", {
				name: "Import Preview",
			});
			expect(mocks.getManagedImportBatch).toHaveBeenCalledWith(BATCH_ID);
			expect(within(preview).getByText("One More Time")).toBeTruthy();
			expect(
				(
					screen.getByRole("button", {
						name: "Confirm Import",
					}) as HTMLButtonElement
				).disabled,
			).toBe(false);
		});

		it("never cancels the acquisition when the dialog closes", async () => {
			const { onOpenChange } = renderImportDialog();
			fireEvent.click(
				await screen.findByRole("button", { name: "Review import" }),
			);
			await screen.findByRole("region", { name: "Import Preview" });

			fireEvent.click(
				screen.getByRole("button", { name: "Close Import Music" }),
			);

			await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
			expect(window.confirm).not.toHaveBeenCalled();
			expect(mocks.cancelSoulseekAcquisition).not.toHaveBeenCalled();
			expect(mocks.cancelManagedImportBatch).not.toHaveBeenCalled();
		});

		it("cancels the acquisition, not just its batch, from the review", async () => {
			const { onOpenChange } = renderImportDialog();
			fireEvent.click(
				await screen.findByRole("button", { name: "Review import" }),
			);
			await screen.findByRole("region", { name: "Import Preview" });

			fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

			await waitFor(() =>
				expect(mocks.cancelSoulseekAcquisition).toHaveBeenCalledWith("acq-1"),
			);
			expect(mocks.cancelManagedImportBatch).not.toHaveBeenCalled();
			await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
		});
	});

	describe("review of a failed acquisition", () => {
		beforeEach(() => {
			mocks.listSoulseekAcquisitions.mockResolvedValue({
				items: [
					acquisition({
						state: "failed",
						batchId: BATCH_ID,
						failureReason:
							"No source of this version is left: 1 of 2 files did not arrive.",
					}),
				],
			});
			mocks.getManagedImportBatch.mockResolvedValue(reviewBatch);
			mocks.confirmManagedImportBatch.mockReturnValue(new Promise(() => {}));
		});

		it("offers to review the files that arrived, or dismiss them", async () => {
			renderImportDialog();

			const row = await screen.findByRole("listitem", {
				name: "Daft Punk Discovery",
			});
			expect(row.textContent).toContain("1 of 2 files did not arrive.");
			expect(row.textContent).toContain(
				"Review the files that arrived, or choose another version with Install.",
			);
			expect(
				within(row).getByRole("button", { name: "Review import" }),
			).toBeTruthy();
			expect(
				within(row).getByRole("button", {
					name: "Dismiss Daft Punk Discovery",
				}),
			).toBeTruthy();
		});

		it("confirms the arrived files through the Import Preview", async () => {
			renderImportDialog();
			fireEvent.click(
				await screen.findByRole("button", { name: "Review import" }),
			);
			const preview = await screen.findByRole("region", {
				name: "Import Preview",
			});
			expect(mocks.getManagedImportBatch).toHaveBeenCalledWith(BATCH_ID);
			expect(within(preview).getByText("One More Time")).toBeTruthy();

			fireEvent.click(screen.getByRole("button", { name: "Confirm Import" }));

			await waitFor(() =>
				expect(mocks.confirmManagedImportBatch).toHaveBeenCalledWith(
					BATCH_ID,
					3,
					["job-1"],
					undefined,
					undefined,
				),
			);
		});
	});
});
