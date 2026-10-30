import { expect, type Page, test } from "@playwright/test";

// A real Guest needs the Guest Listener, a second server, so these run the
// real client against mocked Listen Together responses.
const room = {
	revision: 1,
	trackIds: [],
	currentIndex: null,
	playing: false,
	positionMs: 0,
	positionAtMs: 0,
	lyricsOpen: false,
	guestCount: 1,
	maxGuests: 8,
	expiresAt: "2030-01-01T12:00:00Z",
};
const INVITE_URL = "https://music.example.com/join#invite";

type Role = "owner" | "guest" | "none";

async function mockListenTogether(page: Page, initialRole: Role) {
	const state = { role: initialRole, isRoomOpen: initialRole === "guest" };
	await page.route("**/api/v1/health", (route) =>
		route.fulfill({
			json: {
				status: "ok",
				version: "e2e",
				capabilities: ["api.v1", "listen-together.v1"],
				dependencies: [],
			},
		}),
	);
	await page.route("**/api/v1/listen-together/session", (route) =>
		route.fulfill({ json: { role: state.role } }),
	);
	await page.route("**/api/v1/listen-together/room/events", (route) =>
		route.fulfill({
			status: 200,
			contentType: "text/event-stream",
			body: `event: room-state\ndata: ${JSON.stringify(room)}\n\n`,
		}),
	);
	await page.route("**/api/v1/listen-together/room/clock", (route) =>
		route.fulfill({ json: { serverTimeMs: Date.now() } }),
	);
	await page.route("**/api/v1/listen-together/room", (route) => {
		const ownerRoom = { ...room, inviteUrl: INVITE_URL };
		switch (route.request().method()) {
			case "POST":
				state.isRoomOpen = true;
				return route.fulfill({ status: 201, json: ownerRoom });
			case "DELETE":
				state.isRoomOpen = false;
				return route.fulfill({ status: 204 });
			default:
				return state.isRoomOpen
					? route.fulfill({
							json: state.role === "owner" ? ownerRoom : room,
						})
					: route.fulfill({
							status: 404,
							json: {
								error: "not found",
								code: "room_not_found",
								message: "No Room is open",
							},
						});
		}
	});
	await page.route("**/api/v1/listen-together/join", async (route) => {
		const { token } = route.request().postDataJSON() as { token: string };
		if (token === "full") {
			return route.fulfill({
				status: 409,
				json: {
					error: "conflict",
					code: "room_full",
					message: "This Room is full",
				},
			});
		}
		state.role = "guest";
		state.isRoomOpen = true;
		return route.fulfill({ json: room });
	});
	await page.route("**/api/v1/listen-together/leave", (route) => {
		state.role = "none";
		state.isRoomOpen = false;
		return route.fulfill({ status: 204 });
	});
}

function trackApiCalls(page: Page): string[] {
	const calls: string[] = [];
	page.on("request", (request) => {
		const { pathname } = new URL(request.url());
		if (pathname.startsWith("/api/")) calls.push(pathname);
	});
	return calls;
}

test.describe("the Owner", () => {
	test("opens a Room and closes it", async ({ page }) => {
		await mockListenTogether(page, "owner");
		await page.goto("/listen-together");

		await page
			.getByRole("button", { name: "Listen with friends in my library" })
			.click();
		await expect(page.getByLabel("Invite Link")).toHaveValue(INVITE_URL);
		await expect(page.getByText("1 of 8")).toBeVisible();
		await expect(page.getByText("Room open")).toBeVisible();
		await expect(
			page.getByRole("button", { name: "Connect to a friend's library" }),
		).toHaveCount(0);

		await page.getByRole("button", { name: "Close room" }).click();
		await page
			.getByRole("dialog")
			.getByRole("button", { name: "Close room" })
			.click();
		await expect(
			page.getByRole("button", { name: "Listen with friends in my library" }),
		).toBeVisible();
		await expect(page.getByText("Room open")).toHaveCount(0);
	});

	test("reaches the page from the left bar", async ({ page }) => {
		await mockListenTogether(page, "owner");
		await page.goto("/library/albums");
		await page.getByRole("link", { name: "Listen Together" }).click();
		await expect(page).toHaveURL(/\/listen-together/);
	});
});

test.describe("a visitor without a session", () => {
	test("sees only the invite notice and asks for no library", async ({
		page,
	}) => {
		await mockListenTogether(page, "none");
		const calls = trackApiCalls(page);
		await page.goto("/library/albums");

		await expect(
			page.getByRole("heading", { name: "You need an invite link" }),
		).toBeVisible();
		await expect(page.getByRole("navigation")).toHaveCount(0);
		expect(new Set(calls)).toEqual(
			new Set(["/api/v1/listen-together/session", "/api/v1/health"]),
		);
	});

	test("joins from the invite and lands in the Guest shell", async ({
		page,
	}) => {
		await mockListenTogether(page, "none");
		await page.goto("/join#invite-token");

		const join = page.getByRole("button", { name: "Join" });
		await expect(join).toBeVisible();
		expect(new URL(page.url()).hash).toBe("");
		expect(page.url()).not.toContain("invite-token");

		await join.click();

		await expect(page).toHaveURL(/\/library\/albums$/);
		await expect(
			page.getByText("Owner's library", { exact: true }),
		).toBeVisible();
		for (const name of ["Radio", "Playlists", "Listen Together"]) {
			await expect(page.getByRole("link", { name })).toHaveCount(0);
		}

		await page.getByRole("button", { name: "Leave" }).click();
		await expect(
			page.getByRole("heading", { name: "You left the room" }),
		).toBeVisible();
		await expect(page.getByRole("navigation")).toHaveCount(0);
	});

	test("is told when the Room is full", async ({ page }) => {
		await mockListenTogether(page, "none");
		await page.goto("/join#full");

		await page.getByRole("button", { name: "Join" }).click();

		await expect(page.getByRole("alert")).toHaveText("This room is full");
	});
});

test("a Guest opening a blocked route returns to the library", async ({
	page,
}) => {
	await mockListenTogether(page, "guest");
	await page.goto("/playlists");

	await expect(page).toHaveURL(/\/library\/albums$/);
	await expect(
		page.getByText("Owner's library", { exact: true }),
	).toBeVisible();
});
