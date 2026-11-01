import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
} from "../listen-together/ListenTogetherContext";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import {
	PlaybackProvider,
	type PlaybackApi,
} from "../playback/PlaybackProvider";
import { LayoutProvider } from "./LayoutProvider";
import { PhoneTabBar, phoneNav } from "./PhoneShell";
import { PlayerBar } from "./PlayerBar";

let pathname = "/library/albums";

vi.mock("@tanstack/react-router", () => ({
	Link: ({ to, children }: { to: string; children: ReactNode }) => (
		<a href={to}>{children}</a>
	),
	useLocation: () => ({ pathname }),
	useNavigate: () => vi.fn(),
}));

const labels = (links: { label: string }[]) => links.map(({ label }) => label);

function renderTabBar(overrides: Partial<ListenTogetherContextValue> = {}) {
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
			<PhoneTabBar />
		</ListenTogetherContext.Provider>,
	);
}

describe("phoneNav", () => {
	it("gives the Owner four tabs and the rest under More", () => {
		const { tabs, more } = phoneNav("owner");
		expect(labels(tabs)).toEqual(["Albums", "Artists", "Playlists", "Radio"]);
		expect(labels(more)).toEqual([
			"Tracks",
			"Genres",
			"Favorites",
			"Listen Together",
			"Settings",
		]);
	});

	it("hides what a Guest may not use", () => {
		const { tabs, more } = phoneNav("guest");
		expect(labels(tabs)).toEqual(["Albums", "Artists"]);
		expect(labels(more)).toEqual(["Tracks", "Genres", "Settings"]);
	});
});

describe("PhoneTabBar", () => {
	afterEach(() => {
		cleanup();
		pathname = "/library/albums";
	});

	it("opens More, closes it on Escape and returns focus to the tab", () => {
		renderTabBar();
		const more = screen.getByRole("button", { name: "More" });
		more.focus();
		fireEvent.click(more);
		expect(screen.getByRole("dialog", { name: "More" })).toBeTruthy();
		expect(
			screen.getByRole("link", { name: "Settings" }).getAttribute("href"),
		).toBe("/settings");
		fireEvent.keyDown(document, { key: "Escape" });
		expect(screen.queryByRole("dialog", { name: "More" })).toBeNull();
		expect(document.activeElement).toBe(more);
	});

	it("lights More on a route it holds", () => {
		pathname = "/library/tracks";
		renderTabBar();
		expect(
			screen.getByRole("button", { name: "More" }).classList.contains("active"),
		).toBe(true);
	});
});

describe("PlayerBar in phone mode", () => {
	afterEach(() => {
		cleanup();
		vi.unstubAllGlobals();
	});

	it("shows no mini player idle and never writes Layout Preferences", () => {
		vi.stubGlobal("matchMedia", () => ({
			matches: true,
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
		}));
		const onPreferencesChange = vi.fn();
		const { container } = render(
			<LayoutProvider onPreferencesChange={onPreferencesChange}>
				<PlaybackProvider
					api={
						{
							getQueue: async () => ({ items: [], revision: "0" }),
						} as unknown as PlaybackApi
					}
					engine={new InMemoryPlaybackEngine()}
				>
					<PlayerBar />
				</PlaybackProvider>
			</LayoutProvider>,
		);
		expect(container.querySelector("[data-mini-player]")).toBeNull();
		fireEvent.keyDown(document.body, { key: "q" });
		expect(onPreferencesChange).not.toHaveBeenCalled();
	});
});
