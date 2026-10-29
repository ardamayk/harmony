import { expect, test } from "@playwright/test";

test("albums page loads", async ({ page }) => {
	await page.goto("/library/albums");
	await expect(page.getByRole("heading", { name: "Albums" })).toBeVisible();
	await expect(
		page
			.getByRole("navigation")
			.filter({ has: page.getByRole("link", { name: "Albums" }) }),
	).toBeVisible();
});

test("navigation links render", async ({ page }) => {
	await page.goto("/library/albums");
	await expect(page.getByRole("link", { name: "Albums" })).toBeVisible();
	await expect(page.getByRole("link", { name: "Artists" })).toBeVisible();
	await expect(page.getByRole("link", { name: "Genres" })).toBeVisible();
	await expect(page.getByRole("link", { name: "Settings" })).toBeVisible();
	const queueHeading = page.getByRole("heading", { name: "Queue" });
	if (!(await queueHeading.isVisible())) {
		await page.getByRole("button", { name: "Toggle queue panel" }).click();
	}
	await expect(queueHeading).toBeVisible();
});

test("albums page shows filters", async ({ page }) => {
	await page.goto("/library/albums");
	await page.getByRole("button", { name: "Filters" }).click();
	await expect(
		page.getByRole("dialog", { name: "Album filters" }),
	).toBeVisible();
});

test("artists page loads from the top nav", async ({ page }) => {
	await page.goto("/library/albums");
	await page.getByRole("link", { name: "Artists" }).click();
	await expect(page).toHaveURL(/\/library\/artists/);
	await expect(page.getByRole("heading", { name: "Artists" })).toBeVisible();
});

test("library search opens from the top nav and groups results", async ({
	page,
}) => {
	await page.route("**/api/v1/library/search?*", (route) =>
		route.fulfill({
			json: {
				tracks: [],
				albums: [],
				artists: [
					{ type: "artist", id: "artist-a", name: "A", match: "direct" },
				],
				genres: [],
				playlists: [],
			},
		}),
	);
	await page.goto("/library/albums");
	await page.getByRole("button", { name: "Search your library" }).click();
	const dialog = page.getByTestId("library-search-dialog");
	await expect(dialog).toBeVisible();
	await dialog.getByRole("combobox").fill("a");
	await expect(dialog.getByText("From Your Library")).toBeVisible();
});

test("now playing widget shows empty state", async ({ page }) => {
	await page.goto("/library/albums");
	await expect(page.getByText("Nothing playing").first()).toBeVisible();
});

test("root redirects to albums", async ({ page }) => {
	await page.goto("/");
	await expect(page).toHaveURL(/\/library\/albums/);
});

test("settings theme preset buttons", async ({ page }) => {
	await page.goto("/settings");
	await expect(page.getByRole("button", { name: "Earthly" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Vintage Harbor" }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Sage Hearth" })).toBeVisible();
});

test("product docs load through server module", async ({ page }) => {
	await page.goto("/docs/");
	await expect(
		page.getByRole("heading", { name: "Navidrome Replacement" }),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "API Reference" })).toBeVisible();
});

// Scalar is self-hosted (ADR 0035): the reference renders the spec, and
// nothing it loads leaves the page's origin (no CDN, fonts or telemetry).
test("API reference loads through docs module without leaving the origin", async ({
	page,
	baseURL,
}) => {
	const origin = new URL(baseURL ?? "http://localhost:3000").origin;
	const foreign: string[] = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (
			url.protocol !== "data:" &&
			url.protocol !== "blob:" &&
			url.origin !== origin
		) {
			foreign.push(request.url());
		}
	});
	await page.goto("/api/docs");
	await expect(page).toHaveTitle("Earthly Audio API Reference");
	await expect(
		page.getByRole("heading", { level: 1, name: "Navidrome Replacement API" }),
	).toBeVisible();
	await expect(page.getByText("/api/v1/health").first()).toBeVisible();
	await page.waitForLoadState("networkidle");
	expect(foreign).toEqual([]);
});

// The page makes room for the pinned Queue Drawer at every desktop width,
// down to the 900px minimum window; phone widths are not a target.
test("queue reserves space beside the page on narrow and wide desktop windows", async ({
	page,
}) => {
	await page.goto("/library/albums");
	const drawer = page.locator("[data-queue-drawer]");
	if ((await drawer.getAttribute("data-state")) !== "open") {
		await page.getByRole("button", { name: "Toggle queue panel" }).click();
	}
	await expect(drawer).toHaveAttribute("data-state", "open");
	const main = page.locator("main");
	const layout = () =>
		main.evaluate((element) => {
			const style = getComputedStyle(element);
			const left = Number.parseFloat(style.paddingLeft);
			const right = Number.parseFloat(style.paddingRight);
			return { right, content: element.clientWidth - left - right };
		});
	for (const width of [900, 1280]) {
		await page.setViewportSize({ width, height: 800 });
		await expect.poll(async () => (await layout()).right).toBeGreaterThan(250);
		await expect
			.poll(async () => (await layout()).content)
			.toBeGreaterThan(300);
	}
});

test("library search drops down over the top bar field and animates result height", async ({
	page,
}) => {
	await page.route("**/api/v1/library/search?*", (route) =>
		route.fulfill({
			json: {
				tracks: Array.from({ length: 5 }, (_, i) => ({
					type: "track",
					id: `track-${i}`,
					name: `Track ${i}`,
					match: "direct",
				})),
				albums: Array.from({ length: 5 }, (_, i) => ({
					type: "album",
					id: `album-${i}`,
					name: `Album ${i}`,
					match: "direct",
				})),
				artists: [],
				genres: [],
				playlists: [],
			},
		}),
	);
	await page.goto("/library/albums");
	await page.setViewportSize({ width: 2800, height: 700 });
	const drawer = page.locator("[data-queue-drawer]");
	const card = page.getByTestId("library-search-dialog");
	const opener = page.getByRole("button", { name: "Search your library" });
	// The open modal hides the page from the accessibility tree, so the field
	// is measured by its attribute.
	const field = page.locator("[data-library-search-field]");
	// ADR 0028: the card drops down exactly over the top bar's field.
	const expectOverField = () =>
		expect
			.poll(async () => {
				const box = await card.boundingBox();
				const under = await field.boundingBox();
				return box && under
					? [box.x - under.x, box.y - under.y, box.width - under.width].map(
							Math.round,
						)
					: null;
			})
			.toEqual([0, 0, 0]);
	for (const open of [true, false]) {
		if (((await drawer.getAttribute("data-state")) === "open") !== open)
			await page.getByRole("button", { name: "Toggle queue panel" }).click();
		await opener.click();
		await expect(card).toHaveCSS("animation-name", "library-search-enter");
		await expectOverField();
		if (open) {
			await expect
				.poll(async () => {
					const box = await card.boundingBox();
					const queue = await drawer.boundingBox();
					return box && queue ? queue.x - box.x - box.width : 0;
				})
				.toBeGreaterThan(24);
		}
		// Viewport less the card's top, the Player Bar's clearance and a 24px gap.
		const maxHeight = await card.evaluate((element) =>
			Number.parseFloat(getComputedStyle(element).maxHeight),
		);
		const input = page.getByRole("combobox");
		await card.evaluate((element) =>
			Promise.all(
				element.getAnimations().map((animation) => animation.finished),
			),
		);
		const initialHeight = (await card.boundingBox())?.height ?? 0;
		await input.fill("ta");
		await expect(card.getByRole("option")).toHaveCount(10);
		// Pause the real CSS transition midway: layout must be between both sizes.
		const halfway = await card.evaluate(async (element) => {
			for (let frame = 0; frame < 30; frame++) {
				const transition = element
					.getAnimations()
					.find(
						(animation) =>
							animation instanceof CSSTransition &&
							animation.transitionProperty === "height",
					);
				if (transition) {
					transition.pause();
					transition.currentTime =
						Number(transition.effect?.getTiming().duration) / 2;
					const height = element.getBoundingClientRect().height;
					transition.finish();
					return height;
				}
				await new Promise(requestAnimationFrame);
			}
			return 0;
		});
		await expect
			.poll(async () => (await card.boundingBox())?.height ?? 0)
			.toBe(maxHeight);
		expect(halfway).toBeGreaterThan(initialHeight);
		expect(halfway).toBeLessThan(maxHeight);
		await input.press("ArrowUp");
		await expect
			.poll(() =>
				card.getByRole("listbox").evaluate((element) => element.scrollTop),
			)
			.toBeGreaterThan(0);
		await input.fill("");
		await expect
			.poll(async () => (await card.boundingBox())?.height ?? 0)
			.toBe(initialHeight);
		await input.press("Escape");
	}
	// The narrowest supported width (design rules: Widths).
	await page.setViewportSize({ width: 640, height: 812 });
	await page.emulateMedia({ reducedMotion: "reduce" });
	await opener.click();
	await expect(card).toHaveCSS("animation-name", "none");
	await expect(card).toHaveCSS("transition-duration", "0s");
	await expectOverField();
});

test("library search restores its opener after closing and reopening", async ({
	page,
}) => {
	await page.goto("/library/albums");
	const opener = page.getByRole("button", { name: "Search your library" });
	for (let attempt = 0; attempt < 2; attempt += 1) {
		await opener.click();
		await expect(page.getByRole("combobox")).toBeFocused();
		await page.getByRole("combobox").press("Escape");
		await expect(opener).toBeFocused();
	}
});
