import type { ListenTogetherRoom } from "@repo/api-client";
import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
} from "@repo/ui";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	type FakeDesktopBridge,
	installDesktopBridge,
} from "#/desktop/testing";
import { ListenTogetherPage } from "./-listen-together-page";

const PLACEHOLDER = "Connect to a friend's library";

const room: ListenTogetherRoom = {
	revision: 1,
	trackIds: [],
	currentIndex: null,
	playing: false,
	positionMs: 0,
	positionAtMs: 0,
	lyricsOpen: false,
	guestCount: 2,
	maxGuests: 8,
	expiresAt: "2026-11-01T12:00:00Z",
	inviteUrl: "https://music.example.com/join#token",
};

function value(
	overrides: Partial<ListenTogetherContextValue> = {},
): ListenTogetherContextValue {
	return {
		role: "owner",
		isAvailable: true,
		room: null,
		ending: null,
		openRoom: vi.fn(async () => {}),
		closeRoom: vi.fn(async () => {}),
		leaveRoom: vi.fn(async () => {}),
		joinRoom: vi.fn(async () => {}),
		...overrides,
	};
}

function renderPage(context: ListenTogetherContextValue) {
	return render(
		<ListenTogetherContext.Provider value={context}>
			<ListenTogetherPage />
		</ListenTogetherContext.Provider>,
	);
}

describe("Listen Together page", () => {
	let desktop: FakeDesktopBridge | undefined;

	afterEach(() => {
		cleanup();
		desktop?.uninstall();
		desktop = undefined;
		vi.restoreAllMocks();
	});

	it("opens a Room from the Owner's action", async () => {
		const context = value();
		renderPage(context);

		fireEvent.click(
			screen.getByRole("button", {
				name: "Listen with friends in my library",
			}),
		);

		await waitFor(() => expect(context.openRoom).toHaveBeenCalledOnce());
	});

	it("reports a Room that could not be opened", async () => {
		vi.spyOn(console, "warn").mockImplementation(() => {});
		renderPage(
			value({
				openRoom: vi.fn(async () => {
					throw new Error("down");
				}),
			}),
		);

		fireEvent.click(
			screen.getByRole("button", {
				name: "Listen with friends in my library",
			}),
		);

		expect((await screen.findByRole("alert")).textContent).toContain(
			"Couldn't open the Room",
		);
	});

	it("explains the Guest Listener instead of offering a link when unavailable", () => {
		const context = value({ isAvailable: false });
		renderPage(context);

		expect(
			screen.queryByRole("button", {
				name: "Listen with friends in my library",
			}),
		).toBeNull();
		expect(screen.getByText(/GUEST_ADDR and PUBLIC_URL/)).toBeTruthy();
		expect(
			screen
				.getByRole("link", { name: "Read how to set up Listen Together" })
				.getAttribute("href"),
		).toBe("/docs/#listen-together");
	});

	it("shows an open Room's link, Guests, expiry and warning", () => {
		renderPage(value({ room }));

		expect(
			(screen.getByLabelText("Invite Link") as HTMLInputElement).value,
		).toBe(room.inviteUrl);
		expect(screen.getByText("2 of 8")).toBeTruthy();
		expect(screen.getByText("Expires")).toBeTruthy();
		expect(screen.getByText(/share it like a password/)).toBeTruthy();
		expect(
			screen.queryByRole("button", {
				name: "Listen with friends in my library",
			}),
		).toBeNull();
	});

	it("copies the Invite Link", async () => {
		const writeText = vi.fn(async () => {});
		Object.defineProperty(navigator, "clipboard", {
			configurable: true,
			value: { writeText },
		});
		renderPage(value({ room }));

		fireEvent.click(screen.getByRole("button", { name: "Copy" }));

		await waitFor(() => expect(writeText).toHaveBeenCalledWith(room.inviteUrl));
	});

	it("closes the Room only after confirmation", async () => {
		const context = value({ room });
		renderPage(context);

		fireEvent.click(screen.getByRole("button", { name: "Close room" }));
		expect(context.closeRoom).not.toHaveBeenCalled();

		const dialog = await screen.findByRole("dialog");
		expect(dialog.textContent).toContain("the Invite Link stops working");
		const [, confirm] = screen.getAllByRole("button", {
			name: "Close room",
			hidden: true,
		});
		fireEvent.click(confirm ?? dialog);

		await waitFor(() => expect(context.closeRoom).toHaveBeenCalledOnce());
	});

	it("does not show the Desktop placeholder in the Web Client", () => {
		renderPage(value());
		expect(screen.queryByRole("button", { name: PLACEHOLDER })).toBeNull();
		cleanup();
		renderPage(value({ room }));
		expect(screen.queryByRole("button", { name: PLACEHOLDER })).toBeNull();
	});

	it("shows the Desktop placeholder disabled and not clickable", () => {
		desktop = installDesktopBridge();
		const context = value({ room });
		renderPage(context);

		const button = screen.getByRole("button", {
			name: PLACEHOLDER,
		}) as HTMLButtonElement;
		expect(button.disabled).toBe(true);
		fireEvent.click(button);
		for (const action of [
			context.joinRoom,
			context.openRoom,
			context.closeRoom,
		]) {
			expect(action).not.toHaveBeenCalled();
		}
	});
});
