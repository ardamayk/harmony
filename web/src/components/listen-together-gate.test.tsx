import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
} from "@repo/ui";
import { useHiddenFromGuest } from "@repo/ui/guest-access";
import {
	createRootRoute,
	createRoute,
	createRouter,
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
import { GuestBar } from "./guest-bar";
import {
	ListenTogetherGate,
	resolveListenTogetherView,
} from "./listen-together-gate";

function value(
	overrides: Partial<ListenTogetherContextValue>,
): ListenTogetherContextValue {
	return {
		role: "owner",
		isAvailable: true,
		room: null,
		ending: null,
		openRoom: vi.fn(),
		closeRoom: vi.fn(),
		leaveRoom: vi.fn(async () => {}),
		joinRoom: vi.fn(),
		...overrides,
	};
}

/** Stands in for the library shell, which fetches as soon as it mounts. */
function Library({ onMount }: { onMount: () => void }) {
	onMount();
	return <h1>Library</h1>;
}

async function renderGate(
	context: ListenTogetherContextValue,
	path: string,
	onLibraryMount: () => void = () => {},
) {
	window.history.replaceState(null, "", path);
	const rootRoute = createRootRoute({
		component: () => (
			<ListenTogetherGate>
				<Library onMount={onLibraryMount} />
			</ListenTogetherGate>
		),
	});
	const paths = ["/join", "/library/albums", "/playlists", "/listen-together"];
	const router = createRouter({
		routeTree: rootRoute.addChildren(
			paths.map((routePath) =>
				createRoute({
					getParentRoute: () => rootRoute,
					path: routePath,
					component: () => <h2>Route {routePath}</h2>,
				}),
			),
		),
	});
	await act(async () => {
		render(
			<ListenTogetherContext.Provider value={context}>
				<RouterProvider router={router} />
			</ListenTogetherContext.Provider>,
		);
	});
	return router;
}

describe("resolveListenTogetherView", () => {
	it("gives the Owner the library, whatever ended", () => {
		expect(resolveListenTogetherView("owner", null, "/settings")).toEqual({
			kind: "app",
		});
		expect(resolveListenTogetherView("owner", "closed", "/settings")).toEqual({
			kind: "app",
		});
	});

	it("shows nothing while the role is unknown", () => {
		for (const path of ["/library/albums", "/join"]) {
			expect(resolveListenTogetherView("loading", null, path)).toEqual({
				kind: "blank",
			});
		}
	});

	it("gives a client without a session only the join page or the notice", () => {
		expect(resolveListenTogetherView("none", null, "/join")).toEqual({
			kind: "join",
		});
		for (const path of ["/", "/library/albums", "/settings", "/nowhere"]) {
			expect(resolveListenTogetherView("none", null, path)).toEqual({
				kind: "needs-invite",
			});
		}
	});

	it("ends a Guest's visit on every route", () => {
		for (const role of ["guest", "none", "loading"] as const) {
			for (const ending of ["closed", "left"] as const) {
				expect(
					resolveListenTogetherView(role, ending, "/library/albums"),
				).toEqual({ kind: "ended", ending });
			}
		}
	});

	it("sends a Guest away from routes they may not use", () => {
		for (const path of [
			"/join",
			"/playlists",
			"/playlists/1",
			"/radio",
			"/radio/discover",
			"/favorites",
			"/listen-together",
		]) {
			expect(resolveListenTogetherView("guest", null, path)).toEqual({
				kind: "redirect",
				to: "/library/albums",
			});
		}
		expect(resolveListenTogetherView("guest", null, "/settings")).toEqual({
			kind: "app",
		});
	});
});

describe("ListenTogetherGate", () => {
	afterEach(() => {
		cleanup();
		window.history.replaceState(null, "", "/");
	});

	it("never mounts the library for a client without a session", async () => {
		const libraryMounted = vi.fn();
		await renderGate(
			value({ role: "none" }),
			"/library/albums",
			libraryMounted,
		);

		expect(
			await screen.findByRole("heading", { name: "You need an invite link" }),
		).toBeTruthy();
		expect(libraryMounted).not.toHaveBeenCalled();
	});

	it("shows the join route bare, without the library", async () => {
		const libraryMounted = vi.fn();
		await renderGate(value({ role: "none" }), "/join", libraryMounted);

		expect(await screen.findByText("Route /join")).toBeTruthy();
		expect(libraryMounted).not.toHaveBeenCalled();
	});

	it("does not flash the library while loading", async () => {
		const libraryMounted = vi.fn();
		await renderGate(
			value({ role: "loading" }),
			"/library/albums",
			libraryMounted,
		);

		expect(libraryMounted).not.toHaveBeenCalled();
		expect(screen.queryByRole("heading")).toBeNull();
	});

	it.each([
		["closed", "The room was closed"],
		["left", "You left the room"],
	] as const)("offers no library after %s", async (ending, title) => {
		const libraryMounted = vi.fn();
		await renderGate(
			value({ role: "guest", ending }),
			"/library/albums",
			libraryMounted,
		);

		expect(await screen.findByRole("heading", { name: title })).toBeTruthy();
		expect(screen.queryByRole("link")).toBeNull();
		expect(screen.queryByRole("button")).toBeNull();
		expect(libraryMounted).not.toHaveBeenCalled();
	});

	it("redirects a Guest away from a blocked route", async () => {
		const router = await renderGate(value({ role: "guest" }), "/playlists");

		await waitFor(() =>
			expect(router.state.location.pathname).toBe("/library/albums"),
		);
	});
});

describe("GuestBar", () => {
	afterEach(cleanup);

	it("names the library and leaves the Room", async () => {
		const context = value({ role: "guest" });
		render(
			<ListenTogetherContext.Provider value={context}>
				<GuestBar />
			</ListenTogetherContext.Provider>,
		);

		expect(screen.getByText("Owner's library")).toBeTruthy();
		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "Leave" }));
		});
		expect(context.leaveRoom).toHaveBeenCalledOnce();
	});

	it("is absent for the Owner", () => {
		render(
			<ListenTogetherContext.Provider value={value({ role: "owner" })}>
				<GuestBar />
			</ListenTogetherContext.Provider>,
		);
		expect(screen.queryByRole("button", { name: "Leave" })).toBeNull();
	});
});

describe("useHiddenFromGuest", () => {
	afterEach(cleanup);

	function Probe() {
		return <p>{useHiddenFromGuest("track-deletion") ? "hidden" : "shown"}</p>;
	}

	it("shows everything without a provider, as the Owner's interface", () => {
		render(<Probe />);
		expect(screen.getByText("shown")).toBeTruthy();
	});

	it("follows the role in a provider", () => {
		render(
			<ListenTogetherContext.Provider value={value({ role: "guest" })}>
				<Probe />
			</ListenTogetherContext.Provider>,
		);
		expect(screen.getByText("hidden")).toBeTruthy();
	});
});
