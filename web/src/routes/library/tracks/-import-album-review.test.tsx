import type { ManagedImportAlbumPreview } from "@repo/api-client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { ImportAlbumReview } from "./-import-album-review";

vi.mock("#/lib/api", () => ({
	apiClient: {
		getManagedImportArtworkUrl: (batchId: string, artworkId: string) =>
			`http://127.0.0.1:43129/token/api/v1/import-batches/${batchId}/artwork/${artworkId}`,
	},
}));

const album: ManagedImportAlbumPreview = {
	key: "album-key",
	title: "Hurry Up Tomorrow",
	albumArtists: ["The Weeknd"],
	existingAlbums: [],
	artworks: [
		{
			id: "cover-1",
			jobId: "job-1",
			mediaType: "image/png",
			contentSha256: "cover-hash",
		},
	],
};

function renderReview(overrides: Partial<ManagedImportAlbumPreview> = {}) {
	const onChange = vi.fn();
	render(
		<ImportAlbumReview
			batchId="batch-1"
			albums={[{ ...album, ...overrides }]}
			decisions={{}}
			isBusy={false}
			onChange={onChange}
			onUploaded={async () => {}}
			onUploadStateChange={() => {}}
		/>,
	);
	return onChange;
}

afterEach(cleanup);

describe("import album review", () => {
	it("requires an explicit choice when several covers are available", () => {
		const onChange = renderReview({
			artworks: [...album.artworks, { ...album.artworks[0], id: "cover-2" }],
		});
		const covers = screen.getAllByRole("radio") as HTMLInputElement[];
		expect(covers.every((cover) => !cover.checked)).toBe(true);
		fireEvent.click(
			screen.getByRole("radio", {
				name: `Embedded cover 2 for ${album.title}`,
			}),
		);
		expect(onChange).toHaveBeenCalledWith(
			expect.objectContaining({
				artworkMode: "selected",
				artworkId: "cover-2",
			}),
		);
	});

	it("shows an explicit unavailable preview instead of a broken image", () => {
		renderReview();
		const image = screen.getByRole("img", { name: "Embedded cover 1" });
		expect(image.getAttribute("src")).toBe(
			"http://127.0.0.1:43129/token/api/v1/import-batches/batch-1/artwork/cover-1",
		);
		fireEvent.error(image);
		expect(screen.getByRole("status").textContent).toContain(
			"Preview unavailable",
		);
		expect(screen.queryByRole("img")).toBeNull();
	});

	it("offers no cover when the files contain no artwork", () => {
		renderReview({ artworks: [] });
		expect(screen.getByText(/No embedded cover was found/)).toBeTruthy();
		expect(
			(
				screen.getByRole("radio", {
					name: `No cover for ${album.title}`,
				}) as HTMLInputElement
			).checked,
		).toBe(true);
	});

	it("preserves an existing album cover without offering replacement controls", () => {
		renderReview({
			existingAlbums: [{ id: "existing-1", hasArtwork: true, tracks: [] }],
		});
		expect(
			screen.getByRole("combobox", {
				name: `Album destination for ${album.title}`,
			}),
		).toBeTruthy();
		expect(
			screen.getByText("Existing Album cover will be preserved."),
		).toBeTruthy();
		expect(screen.queryByRole("radio")).toBeNull();
		expect(
			screen.queryByLabelText(`Upload cover for ${album.title}`),
		).toBeNull();
	});
	it("shows the automatic cover selection and lets the user opt out", () => {
		const onChange = renderReview();
		const embedded = screen.getByRole("radio", {
			name: `Embedded cover 1 for ${album.title}`,
		}) as HTMLInputElement;
		expect(embedded.checked).toBe(true);
		fireEvent.click(
			screen.getByRole("radio", { name: `No cover for ${album.title}` }),
		);
		expect(onChange).toHaveBeenCalledWith(
			expect.objectContaining({ artworkMode: "none", artworkId: undefined }),
		);
	});
	it("explains the review and shows a single destination without a redundant selector", () => {
		renderReview();
		expect(screen.getByText("Review albums and covers")).toBeTruthy();
		expect(
			screen.getByText(/Nothing is added to your library until you confirm/),
		).toBeTruthy();
		expect(screen.getByText("Create new album")).toBeTruthy();
		expect(
			screen.getByText(/No library album matches this title and album artist/),
		).toBeTruthy();
		expect(
			screen.queryByRole("combobox", {
				name: `Album destination for ${album.title}`,
			}),
		).toBeNull();
	});
});
