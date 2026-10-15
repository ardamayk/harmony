import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	AppShell,
	defaultPreferences,
	LayoutProvider,
	PlaybackProvider,
	TopNav,
} from "../index";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		className,
		title,
		to,
	}: {
		children: ReactNode;
		className?: string;
		title?: string;
		to: string;
	}) => (
		<a className={className} href={to} title={title}>
			{children}
		</a>
	),
}));

const mockPlaybackApi = {
	getQueue: async () => ({ items: [], revision: "0" }),
	replaceQueue: async () => ({ items: [], revision: "1" }),
	reorderQueue: async () => ({ items: [], revision: "1" }),
	appendQueueItem: async () => ({ items: [], revision: "1" }),
	removeQueueItem: async () => ({ items: [], revision: "1" }),
	clearQueue: async () => ({ items: [], revision: "1" }),
	getStreamUrl: (id: string) => `/stream/${id}`,
	getAlbumCoverUrl: (id: string) => `/cover/${id}`,
	getRadioStationStreamUrl: (id: string) => `/radio/${id}`,
	getRadioCatalogPreviewStreamUrl: (id: string) => `/radio/preview/${id}`,
	getRadioNowPlaying: async () => ({}),
	listPlaylists: async () => ({ items: [], total: 0 }),
	getPlaylist: async (playlistId: string) => ({
		id: playlistId,
		name: "Playlist",
		isDefault: false,
		trackCount: 0,
		tracks: [],
	}),
	createPlaylist: async (name: string) => ({
		id: "playlist-1",
		name,
		isDefault: false,
		trackCount: 0,
	}),
	addPlaylistTrack: async () => ({
		id: "playlist-1",
		name: "Playlist",
		isDefault: false,
		trackCount: 1,
		tracks: [],
	}),
	removePlaylistTrack: async () => ({
		id: "playlist-1",
		name: "Playlist",
		isDefault: false,
		trackCount: 0,
		tracks: [],
	}),
};

const collapsedQueuePreferences = {
	...defaultPreferences,
	layout: {
		...defaultPreferences.layout,
		collapsed: { left: false, right: true },
	},
};

function renderShell(
	ui: ReactNode,
	preferences: typeof defaultPreferences = defaultPreferences,
) {
	return render(
		<LayoutProvider initialPreferences={preferences}>
			<PlaybackProvider
				api={mockPlaybackApi}
				engine={new InMemoryPlaybackEngine()}
			>
				{ui}
			</PlaybackProvider>
		</LayoutProvider>,
	);
}

describe("AppShell", () => {
	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
		window.localStorage.clear();
	});

	it("pins the queue by default and floats it over the page once unpinned", () => {
		const { container } = renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
		);
		const main = container.querySelector("main") as HTMLElement;
		const pin = screen.getByRole("button", { name: "Pin queue" });
		expect(pin.getAttribute("aria-pressed")).toBe("true");
		expect(main.hasAttribute("data-queue-open")).toBe(true);

		fireEvent.click(pin);

		expect(pin.getAttribute("aria-pressed")).toBe("false");
		// The drawer stays open but no longer pushes the page aside.
		const drawer = container.querySelector(
			"[data-queue-drawer]",
		) as HTMLElement;
		expect(drawer.dataset.state).toBe("open");
		expect(main.hasAttribute("data-queue-open")).toBe(false);
		expect(container.querySelector("[data-queue-scrim]")).toBeNull();
	});

	it("remembers the pin choice on this device", () => {
		renderShell(<AppShell />);
		fireEvent.click(screen.getByRole("button", { name: "Pin queue" }));
		cleanup();

		const { container } = renderShell(<AppShell />);

		expect(
			screen
				.getByRole("button", { name: "Pin queue" })
				.getAttribute("aria-pressed"),
		).toBe("false");
		expect(
			(container.querySelector("main") as HTMLElement).hasAttribute(
				"data-queue-open",
			),
		).toBe(false);
	});

	it("renders the top nav, main content and full-height queue", () => {
		renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
		);
		expect(screen.getByText("Main content")).toBeTruthy();
		expect(screen.getByRole("link", { name: "Albums" })).toBeTruthy();
		expect(screen.getByRole("link", { name: "Settings" })).toBeTruthy();
		expect(screen.getByText("Queue")).toBeTruthy();
		expect(screen.queryByText("Discover (coming soon)")).toBeNull();
	});

	it("has no side columns: the nav is a top bar and the queue a drawer", () => {
		const { container } = renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
		);
		expect(container.querySelector("header")).toBeTruthy();
		expect(container.querySelector("[data-queue-column]")).toBeNull();
		expect(container.querySelector("[data-queue-drawer]")).toBeTruthy();
		expect(
			container.querySelectorAll('[data-slot="resizable-handle"]'),
		).toHaveLength(0);
	});

	it("aligns the floating player dock with the shared page inset", () => {
		const { container } = renderShell(
			<AppShell bottom={<div>Player</div>}>
				<div>Main content</div>
			</AppShell>,
		);
		const scrim = container.querySelector("[data-player-scrim]");
		expect(scrim?.className).toContain("bg-gradient-to-t");
		expect(scrim?.className).toContain("pointer-events-none");
		const dock = container.querySelector("[data-player-dock]");
		const column = container.querySelector("[data-player-dock-column]");
		expect(dock?.className).toContain("inset-x-0");
		// Mirrors PAGE_CONTENT_PADDING_CLASS in web: the same --shell-inset.
		expect(dock?.className).toContain("px-[var(--shell-inset,2rem)]");
		expect(column?.className).toContain("w-full");
		expect(column?.className).not.toContain("max-w-");
	});

	it("opens the queue drawer and makes room for it in the page", () => {
		const { container } = renderShell(
			<AppShell bottom={<div>Player</div>}>
				<div>Main content</div>
			</AppShell>,
		);
		const drawer = container.querySelector(
			"[data-queue-drawer]",
		) as HTMLElement;
		expect(drawer.dataset.state).toBe("open");
		expect(drawer.getAttribute("aria-hidden")).toBe("false");
		expect(drawer.className).toContain("translate-y-0");
		expect(drawer.className).not.toContain("invisible");
		expect(drawer.style.right).toBe("var(--shell-inset, 2rem)");
		// Parks above the Player Bar rather than touching it.
		expect(drawer.style.bottom).toContain("86px");
		const main = container.querySelector("main") as HTMLElement;
		expect(main.hasAttribute("data-queue-open")).toBe(true);
		expect(main.style.paddingRight).toBe("");
		expect(main.className).toContain(
			"data-[queue-open]:pr-[var(--queue-drawer-clearance)]",
		);
	});

	it("keeps the drawer off screen and inert when the queue is collapsed", () => {
		const { container } = renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
			collapsedQueuePreferences,
		);
		const drawer = container.querySelector(
			"[data-queue-drawer]",
		) as HTMLElement;
		expect(drawer.dataset.state).toBe("closed");
		expect(drawer.getAttribute("aria-hidden")).toBe("true");
		expect(drawer.hasAttribute("inert")).toBe(true);
		expect(drawer.className).toContain("translate-y-[calc(100%+8rem)]");
		expect(drawer.className).toContain("invisible");
		const main = container.querySelector("main") as HTMLElement;
		expect(main.hasAttribute("data-queue-open")).toBe(false);
		expect(main.style.paddingRight).toBe("");
		expect(screen.getByText("Main content")).toBeTruthy();
	});

	it("closes the drawer from its own close button", () => {
		const { container } = renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
		);
		fireEvent.click(screen.getByRole("button", { name: "Hide queue" }));
		const drawer = container.querySelector(
			"[data-queue-drawer]",
		) as HTMLElement;
		expect(drawer.dataset.state).toBe("closed");
	});

	it("keeps the independent drawer width without a widget dock", () => {
		const { container } = renderShell(<AppShell bottom={<div>Player</div>} />);
		const drawer = container.querySelector(
			"[data-queue-drawer]",
		) as HTMLElement;
		expect(container.querySelector("[data-widget-dock]")).toBeNull();
		expect(drawer.style.width).toBe("var(--queue-drawer-width, 18rem)");
		expect(drawer.style.bottom).toContain("1.5rem");
		expect(drawer.className).toContain("rounded-2xl");
	});

	it("updates inherited drawer width on shell and window resize without changing the bottom gap", () => {
		let shellWidth = 2150;
		let resizeShell: (() => void) | undefined;
		const disconnect = vi.fn();
		vi.stubGlobal(
			"ResizeObserver",
			class {
				constructor(private callback: () => void) {}
				observe(target: Element) {
					if (target.hasAttribute("data-app-shell"))
						resizeShell = this.callback;
				}
				unobserve() {}
				disconnect = disconnect;
			},
		);
		const getBounds = HTMLElement.prototype.getBoundingClientRect;
		vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
			function (this: HTMLElement) {
				const bounds = getBounds.call(this);
				return this.hasAttribute("data-app-shell")
					? new DOMRect(0, 0, shellWidth, bounds.height)
					: bounds;
			},
		);
		const readStyle = window.getComputedStyle;
		vi.spyOn(window, "getComputedStyle").mockImplementation(
			(element, pseudoElement) => {
				const style = readStyle(element, pseudoElement);
				if (element.tagName === "HEADER") {
					Object.defineProperty(style, "paddingLeft", { value: "24px" });
					// With window controls the Top Nav drops its right inset.
					Object.defineProperty(style, "paddingRight", { value: "0px" });
				}
				if (element === document.documentElement) {
					Object.defineProperty(style, "fontSize", { value: "16px" });
				}
				return style;
			},
		);
		const { container, unmount } = renderShell(
			<AppShell bottom={<div>Player</div>} />,
		);
		const shell = container.querySelector<HTMLElement>("[data-app-shell]");
		const drawer = container.querySelector<HTMLElement>("[data-queue-drawer]");
		const main = container.querySelector("main");
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("518px");
		expect(drawer?.style.getPropertyValue("--queue-drawer-width")).toBe("");
		expect(main?.style.getPropertyValue("--queue-drawer-width")).toBe("");
		expect(drawer?.style.width).toBe("var(--queue-drawer-width, 18rem)");
		expect(main?.style.getPropertyValue("--queue-drawer-clearance")).toBe(
			"calc(var(--queue-drawer-width, 18rem) + 1.5rem)",
		);
		const bottom = drawer?.style.bottom;
		expect(bottom).toBe("calc(16px + 86px + 1.5rem)");
		if (!resizeShell)
			throw new Error("The shell was not observed for resizing");
		shellWidth = 1919;
		act(resizeShell);
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("549px");
		expect(drawer?.style.bottom).toBe(bottom);
		shellWidth = 1000;
		fireEvent(window, new Event("resize"));
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("386px");
		expect(drawer?.style.bottom).toBe(bottom);
		unmount();
		expect(disconnect).toHaveBeenCalled();
	});

	it("lets the Desktop Client drag the window by the top nav's empty space", () => {
		const { container } = render(<TopNav onSearch={vi.fn()} />);
		const header = container.querySelector("header") as HTMLElement;
		expect(header.hasAttribute("data-tauri-drag-region")).toBe(true);
		expect(
			screen
				.getByRole("navigation", { name: "Library" })
				.hasAttribute("data-tauri-drag-region"),
		).toBe(true);
		// Links and buttons stay clickable instead of starting a drag.
		expect(
			screen
				.getByRole("link", { name: "Albums" })
				.hasAttribute("data-tauri-drag-region"),
		).toBe(false);
	});

	it("shows window controls only when the host supplies them", () => {
		const { unmount } = render(<TopNav />);
		expect(screen.queryByRole("group", { name: "Window" })).toBeNull();
		unmount();

		const windowControls = {
			minimize: vi.fn(),
			toggleMaximize: vi.fn(),
			close: vi.fn(),
		};
		render(<TopNav windowControls={windowControls} />);
		fireEvent.click(screen.getByRole("button", { name: "Minimize" }));
		fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
		fireEvent.click(screen.getByRole("button", { name: "Close" }));
		expect(windowControls.minimize).toHaveBeenCalledTimes(1);
		expect(windowControls.toggleMaximize).toHaveBeenCalledTimes(1);
		expect(windowControls.close).toHaveBeenCalledTimes(1);
	});

	it("renders the library sections, Search and Settings in the top nav", () => {
		const onSearch = vi.fn();
		render(<TopNav onSearch={onSearch} />);
		for (const label of [
			"Albums",
			"Artists",
			"Genres",
			"Radio",
			"Tracks",
			"Playlists",
		]) {
			expect(screen.getByRole("link", { name: label })).toBeTruthy();
		}
		fireEvent.click(screen.getByRole("button", { name: "Search" }));
		expect(onSearch).toHaveBeenCalledTimes(1);
		const radioLink = screen.getByRole("link", { name: "Radio" });
		expect(radioLink.className).toContain(
			"[&.active]:bg-[var(--shell-active)]",
		);
		expect(screen.getByRole("link", { name: "Settings" })).toBeTruthy();
		expect(screen.queryByText("Premium Account")).toBeNull();
	});
});
