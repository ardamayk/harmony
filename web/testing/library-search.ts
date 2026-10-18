import { expect, type Page } from "@playwright/test";
import type { LibrarySearchResponse } from "@repo/api-client";

export const searchDialog = (page: Page) =>
	page.getByRole("dialog", { name: "Search your library" });

export async function search(
	page: Page,
	query: string,
): Promise<LibrarySearchResponse> {
	const dialog = searchDialog(page);
	if (!(await dialog.isVisible()))
		await page.getByRole("button", { name: "Search your library" }).click();
	const response = page.waitForResponse((response) => {
		const url = new URL(response.url());
		return (
			url.pathname === "/api/v1/library/search" &&
			url.searchParams.get("q") === query
		);
	});
	await dialog.getByRole("combobox").fill(query);
	const result = await response;
	expect(result.status()).toBe(200);
	const body = (await result.json()) as LibrarySearchResponse;
	const rows = [
		body.bestMatch,
		...body.tracks,
		...body.albums,
		...body.artists,
		...body.genres,
		...body.playlists,
	].filter((row) => row != null);
	await expect
		.poll(async () => {
			const options = await dialog.getByRole("option").evaluateAll((elements) =>
				elements.map((element) => ({
					id: element.id,
					text: element.textContent ?? "",
				})),
			);
			return (
				options.length === rows.length &&
				new Set(options.map((option) => option.id)).size === options.length &&
				rows.every((row, index) => options[index].text.includes(row.name))
			);
		})
		.toBe(true);
	return body;
}
