import { cleanup, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
} from "../listen-together/ListenTogetherContext";
import { Sidebar } from "./Sidebar";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		to,
		title,
		children,
	}: {
		to: string;
		title?: string;
		children: ReactNode;
	}) => (
		<a href={to} title={title}>
			{children}
		</a>
	),
}));

function renderSidebar(overrides: Partial<ListenTogetherContextValue>) {
	const value: ListenTogetherContextValue = {
		role: "owner",
		isAvailable: true,
		room: null,
		ending: null,
		openRoom: vi.fn(),
		closeRoom: vi.fn(),
		leaveRoom: vi.fn(),
		joinRoom: vi.fn(),
		...overrides,
	};
	render(
		<ListenTogetherContext.Provider value={value}>
			<Sidebar />
		</ListenTogetherContext.Provider>,
	);
}

const room = {} as NonNullable<ListenTogetherContextValue["room"]>;

describe("Sidebar", () => {
	afterEach(cleanup);

	it("offers the Owner the Listen Together page", () => {
		renderSidebar({});
		expect(
			screen
				.getByRole("link", { name: "Listen Together" })
				.getAttribute("href"),
		).toBe("/listen-together");
		expect(screen.queryByText("Room open")).toBeNull();
	});

	it("marks an open Room on the entry that leads to it", () => {
		renderSidebar({ room });
		const link = screen.getByRole("link", { name: /Listen Together/ });
		expect(link.textContent).toContain("Room open");
		expect(link.getAttribute("title")).toBe("Listen Together: Room open");
	});

	it("hides what a Guest may not use and keeps the library", () => {
		renderSidebar({ role: "guest", room });
		for (const name of ["Listen Together", "Radio", "Playlists"]) {
			expect(screen.queryByRole("link", { name })).toBeNull();
		}
		for (const name of ["Albums", "Artists", "Genres", "Tracks", "Settings"]) {
			expect(screen.getByRole("link", { name })).toBeTruthy();
		}
		expect(screen.queryByText("Room open")).toBeNull();
	});
});
