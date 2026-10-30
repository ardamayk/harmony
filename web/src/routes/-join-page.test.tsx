import { ApiError } from "@repo/api-client";
import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
} from "@repo/ui";
import {
	createRootRoute,
	createRoute,
	createRouter,
	Outlet,
	RouterProvider,
} from "@tanstack/react-router";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	captureInviteToken,
	clearInviteToken,
	getInviteToken,
} from "#/lib/invite-token";
import {
	INVITE_INVALID_MESSAGE,
	JoinPage,
	joinErrorMessage,
	ROOM_FULL_MESSAGE,
} from "./-join-page";

const TOKEN = "s3cret-invite-token";

function apiError(code: string): ApiError {
	return new ApiError(403, { error: code, code, message: "refused" });
}

function context(
	joinRoom: ListenTogetherContextValue["joinRoom"],
): ListenTogetherContextValue {
	return {
		role: "none",
		isAvailable: true,
		room: null,
		ending: null,
		openRoom: vi.fn(),
		closeRoom: vi.fn(),
		leaveRoom: vi.fn(),
		joinRoom,
	};
}

/** The real route chain, so the token is read before the page renders. */
async function renderJoinRoute(
	joinRoom: ListenTogetherContextValue["joinRoom"],
) {
	const rootRoute = createRootRoute({ component: Outlet });
	const joinRoute = createRoute({
		getParentRoute: () => rootRoute,
		path: "/join",
		beforeLoad: () => captureInviteToken(),
		component: JoinPage,
	});
	const libraryRoute = createRoute({
		getParentRoute: () => rootRoute,
		path: "/library/albums",
		component: () => <h1>Albums</h1>,
	});
	const router = createRouter({
		routeTree: rootRoute.addChildren([joinRoute, libraryRoute]),
	});
	await act(async () => {
		render(
			<ListenTogetherContext.Provider value={context(joinRoom)}>
				<RouterProvider router={router} />
			</ListenTogetherContext.Provider>,
		);
	});
	return router;
}

describe("/join", () => {
	afterEach(() => {
		cleanup();
		clearInviteToken();
		window.history.replaceState(null, "", "/");
		vi.restoreAllMocks();
	});

	it("moves the token from the address bar to memory", () => {
		window.history.replaceState({ kept: true }, "", `/join?a=1#${TOKEN}`);

		captureInviteToken();

		expect(window.location.hash).toBe("");
		expect(window.location.href).not.toContain(TOKEN);
		expect(window.location.pathname + window.location.search).toBe("/join?a=1");
		expect(window.history.state).toEqual({ kept: true });
		expect(getInviteToken()).toBe(TOKEN);
	});

	it("keeps the token through a second capture without a hash", () => {
		window.history.replaceState(null, "", `/join#${TOKEN}`);
		captureInviteToken();
		captureInviteToken();
		expect(getInviteToken()).toBe(TOKEN);
	});

	it("removes the hash on the route and joins from the click", async () => {
		window.history.replaceState(null, "", `/join#${TOKEN}`);
		const joinRoom = vi.fn(async () => {});

		const router = await renderJoinRoute(joinRoom);

		const join = await screen.findByRole("button", { name: "Join" });
		expect(window.location.href).not.toContain(TOKEN);
		expect(router.state.location.href).not.toContain(TOKEN);
		expect(joinRoom).not.toHaveBeenCalled();

		fireEvent.click(join);

		await waitFor(() => expect(joinRoom).toHaveBeenCalledWith(TOKEN));
		expect(await screen.findByRole("heading", { name: "Albums" })).toBeTruthy();
		expect(window.location.pathname).toBe("/library/albums");
		expect(getInviteToken()).toBeNull();
	});

	it("maps the API's refusals to their messages", () => {
		expect(joinErrorMessage(apiError("invite_invalid"))).toBe(
			"This invite link is invalid or has expired",
		);
		expect(joinErrorMessage(apiError("room_full"))).toBe("This room is full");
		expect(joinErrorMessage(new Error("offline"))).toContain("Couldn't reach");
	});

	it("shows a full Room and allows another try, without logging", async () => {
		const logged = [
			vi.spyOn(console, "log"),
			vi.spyOn(console, "warn"),
			vi.spyOn(console, "error"),
		];
		window.history.replaceState(null, "", `/join#${TOKEN}`);
		await renderJoinRoute(
			vi.fn(async () => {
				throw apiError("room_full");
			}),
		);

		fireEvent.click(await screen.findByRole("button", { name: "Join" }));

		expect((await screen.findByRole("alert")).textContent).toBe(
			ROOM_FULL_MESSAGE,
		);
		expect(screen.getByRole("button", { name: "Join" })).toBeTruthy();
		for (const spy of logged) {
			expect(JSON.stringify(spy.mock.calls)).not.toContain(TOKEN);
		}
	});

	it("offers no retry for an invalid invite", async () => {
		window.history.replaceState(null, "", `/join#${TOKEN}`);
		await renderJoinRoute(
			vi.fn(async () => {
				throw apiError("invite_invalid");
			}),
		);

		fireEvent.click(await screen.findByRole("button", { name: "Join" }));

		expect((await screen.findByRole("alert")).textContent).toBe(
			INVITE_INVALID_MESSAGE,
		);
		expect(screen.queryByRole("button", { name: "Join" })).toBeNull();
	});

	it("asks for an invite link when the URL carries no token", async () => {
		window.history.replaceState(null, "", "/join");
		await renderJoinRoute(vi.fn());

		expect(
			await screen.findByRole("heading", { name: "You need an invite link" }),
		).toBeTruthy();
		expect(screen.queryByRole("button", { name: "Join" })).toBeNull();
	});
});
