import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { search, searchDialog } from "../testing/library-search";
import { buildStrictMp3 } from "./fixtures/managed-import-audio";

test("cached absence becomes imported Track; Enter decodes audio; Album navigation and deletion refresh search", async ({
	page,
	request,
}) => {
	test.setTimeout(120_000);
	const suffix = randomUUID();
	const title = `Search Journey Lantern ${suffix}`;
	const album = `Search Journey Collection ${suffix}`;
	const artist = `Search Journey Performer ${suffix}`;
	const guest = `Guest Contributor ${randomUUID()}`;
	const genre = `Search Journey Style ${suffix}`;
	// Observe the real detached Audio element without replacing playback or bytes.
	await page.addInitScript(() => {
		const NativeAudio = window.Audio;
		Object.assign(window, {
			searchAudioEvents: [] as { source: string; time: number }[],
		});
		window.Audio = new Proxy(NativeAudio, {
			construct(target, args) {
				const audio = Reflect.construct(target, args) as HTMLAudioElement;
				audio.addEventListener("timeupdate", () => {
					if (audio.currentTime > 0 && !audio.error) {
						(
							window as unknown as {
								searchAudioEvents: { source: string; time: number }[];
							}
						).searchAudioEvents.push({
							source: audio.currentSrc,
							time: audio.currentTime,
						});
					}
				});
				return audio;
			},
		});
	});
	await page.goto("/library/tracks");
	const absent = await search(page, title);
	expect(absent.bestMatch).toBeUndefined();
	expect(absent.tracks).toEqual([]);
	await page.keyboard.press("Escape");
	await expect(
		page.getByRole("button", { name: "Search your library" }),
	).toBeFocused();

	await page.getByRole("button", { name: "Import Music" }).click();
	const importDialog = page.getByRole("dialog", { name: "Import Music" });
	await importDialog.getByLabel("Audio files").setInputFiles({
		name: "search-journey.mp3",
		mimeType: "audio/mpeg",
		buffer: buildStrictMp3(
			{
				title,
				album,
				albumArtist: artist,
				artists: [`${artist} & ${guest}`],
				track: "1/1",
				genres: [genre],
			},
			{ userText: { ARTISTS: `${artist}\0${guest}` } },
		),
	});
	await expect(
		importDialog.getByText("Accepted", { exact: true }),
	).toBeVisible();
	await importDialog.getByRole("button", { name: "Confirm Import" }).click();
	await expect(
		importDialog.getByText("Imported", { exact: true }),
	).toBeVisible();
	await importDialog.getByRole("button", { name: "Done" }).click();

	const result = await search(page, title);
	expect(result.bestMatch).toMatchObject({ type: "track", name: title });
	const track = result.bestMatch;
	if (!track?.album)
		throw new Error("Imported Track must carry Album identity");
	const best = searchDialog(page).getByRole("group", {
		name: "Best Match",
		exact: true,
	});
	await expect(best.getByRole("option")).toHaveCount(1);
	await expect(
		searchDialog(page)
			.getByRole("option")
			.filter({ has: page.getByText(title, { exact: true }) }),
	).toHaveCount(2);
	const stream = page.waitForResponse(
		(response) =>
			new URL(response.url()).pathname === `/api/v1/tracks/${track.id}/stream`,
	);
	await searchDialog(page).getByRole("combobox").press("Enter");
	expect([200, 206]).toContain((await stream).status());
	await expect(searchDialog(page)).toBeHidden();
	await expect
		.poll(() =>
			page.evaluate(
				(id) =>
					(
						window as unknown as {
							searchAudioEvents: { source: string; time: number }[];
						}
					).searchAudioEvents.some(
						(event) =>
							event.source.includes(`/tracks/${id}/stream`) && event.time > 0,
					),
				track.id,
			),
		)
		.toBe(true);

	const hideQueue = page.getByRole("button", {
		name: "Hide queue",
		exact: true,
	});
	if (await hideQueue.isVisible()) await hideQueue.click();
	const guestResult = await search(page, guest);
	expect(guestResult.bestMatch).toMatchObject({ type: "artist", name: guest });
	expect(guestResult.tracks).toEqual([]);
	const guestID = guestResult.bestMatch?.id;
	if (!guestID) throw new Error("Track-only Artist must have an identity");
	expect(guestResult.artists[0].id).toBe(guestID);
	await searchDialog(page)
		.getByRole("group", { name: "Best Match", exact: true })
		.getByRole("option")
		.click();
	// A track-only Artist opens its own Artist page (artist pages, 19a24db).
	await expect(page).toHaveURL(
		(url) => url.pathname === `/library/artists/${guestID}`,
	);
	await expect(
		page.getByRole("heading", { name: guest, exact: true }),
	).toBeVisible();
	const artistsResponse = await request.get(
		`/api/v1/library/artists?q=${encodeURIComponent(guest)}`,
	);
	expect(artistsResponse.ok()).toBe(true);
	expect((await artistsResponse.json()).items).toEqual([]);
	const albumResult = await search(page, album);
	expect(albumResult.bestMatch).toMatchObject({
		type: "album",
		id: track.album.id,
	});
	await searchDialog(page)
		.getByRole("group", { name: "Best Match", exact: true })
		.getByRole("option")
		.click();
	await expect(page).toHaveURL(new RegExp(`/library/${track.album.id}$`));
	await expect(
		page.getByRole("heading", { name: album, exact: true }),
	).toBeVisible();

	const row = page
		.getByRole("row")
		.filter({ has: page.getByText(title, { exact: true }) });
	await row.click({ button: "right" });
	await page.getByRole("menuitem", { name: "Details", exact: true }).click();
	const details = page.getByRole("dialog", { name: title, exact: true });
	await expect(
		details.getByText("Artist", { exact: true }).locator(".."),
	).toHaveText(`Artist${artist}, ${guest}`);
	await expect(
		details.getByText("Album Artist", { exact: true }).locator(".."),
	).toHaveText(`Album Artist${artist}`);
	await page.keyboard.press("Escape");
	await expect(details).toBeHidden();
	await row.click({ button: "right" });
	await page
		.getByRole("menuitem", { name: "Delete track", exact: true })
		.click();
	const deletion = page.getByRole("dialog", {
		name: `Permanently delete ${title}?`,
	});
	await deletion.getByRole("button", { name: "Delete permanently" }).click();
	await expect(deletion).toBeHidden({ timeout: 15_000 });
	const deleted = await search(page, title);
	expect(deleted.bestMatch).toBeUndefined();
	expect(deleted.tracks).toEqual([]);
	await expect(
		searchDialog(page)
			.getByRole("group", { name: "Library" })
			.getByText("No matches in your library"),
	).toBeVisible();
	expect(
		(await request.get(`/api/v1/tracks/${track.id}/stream`)).status(),
	).toBe(404);
});
