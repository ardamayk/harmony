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
	TopNavActions,
} from "../index";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import { Sidebar } from "./Sidebar";

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
	getExternalTrackPlayback: async (externalTrackId: string) => ({
		format: "progressive" as const,
		streamPath: `/api/v1/external/tracks/${externalTrackId}/stream`,
	}),
	getExternalStreamUrl: (streamPath: string) => `/server${streamPath}`,
	getExternalTrackArtworkUrl: (externalTrackId: string) =>
		`/server/api/v1/external/tracks/${externalTrackId}/artwork`,
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

	it("has a top bar, a Sidebar and a queue drawer, and no resizable columns", () => {
		const { container } = renderShell(
			<AppShell>
				<div>Main content</div>
			</AppShell>,
		);
		expect(container.querySelector("header")).toBeTruthy();
		expect(container.querySelector("[data-sidebar]")).toBeTruthy();
		expect(container.querySelector("[data-queue-column]")).toBeNull();
		expect(container.querySelector("[data-queue-drawer]")).toBeTruthy();
		expect(
			container.querySelectorAll('[data-slot="resizable-handle"]'),
		).toHaveLength(0);
	});

	it("docks the player on the bottom edge, full width", () => {
		const { container } = renderShell(
			<AppShell bottom={<div>Player</div>}>
				<div>Main content</div>
			</AppShell>,
		);
		expect(container.querySelector("[data-player-scrim]")).toBeNull();
		const dock = container.querySelector("[data-player-dock]");
		const column = container.querySelector("[data-player-dock-column]");
		expect(dock?.className).toContain("inset-x-0");
		expect(dock?.className).toContain("bottom-0");
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
		expect(drawer.className).toContain("translate-x-0");
		expect(drawer.className).not.toContain("invisible");
		expect(drawer.className).toContain("right-0");
		// Docked: its bottom edge on the Player Bar's top edge.
		expect(drawer.style.getPropertyValue("--queue-drawer-bottom")).toBe("86px");
		const main = container.querySelector("main") as HTMLElement;
		expect(main.hasAttribute("data-queue-open")).toBe(true);
		expect(main.style.paddingRight).toBe("");
		expect(main.className).toContain(
			"data-[queue-open]:pr-(--queue-drawer-clearance)",
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
		expect(drawer.className).toContain("translate-x-full");
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
		expect(drawer.className).toContain("w-(--queue-drawer-width,18rem)");
		expect(drawer.style.getPropertyValue("--queue-drawer-bottom")).toBe("86px");
		expect(drawer.className).toContain("border-l");
		expect(drawer.className).not.toContain("rounded-lg");
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
		// A 24px page inset and the 240px Sidebar.
		vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(
			function (this: HTMLElement) {
				if (this.hasAttribute("data-shell-inset-probe")) return 24;
				if (this.hasAttribute("data-sidebar")) return 240;
				return 0;
			},
		);
		const readStyle = window.getComputedStyle;
		vi.spyOn(window, "getComputedStyle").mockImplementation(
			(element, pseudoElement) => {
				const style = readStyle(element, pseudoElement);
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
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("400px");
		expect(drawer?.style.getPropertyValue("--queue-drawer-width")).toBe("");
		expect(main?.style.getPropertyValue("--queue-drawer-width")).toBe("");
		expect(drawer?.className).toContain("w-(--queue-drawer-width,18rem)");
		expect(shell?.style.getPropertyValue("--queue-drawer-clearance")).toBe(
			"var(--queue-drawer-width, 18rem)",
		);
		const bottom = drawer?.style.getPropertyValue("--queue-drawer-bottom");
		expect(bottom).toBe("86px");
		if (!resizeShell)
			throw new Error("The shell was not observed for resizing");
		shellWidth = 1919;
		act(resizeShell);
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("400px");
		expect(drawer?.style.getPropertyValue("--queue-drawer-bottom")).toBe(
			bottom,
		);
		shellWidth = 1000;
		fireEvent(window, new Event("resize"));
		expect(shell?.style.getPropertyValue("--queue-drawer-width")).toBe("350px");
		expect(drawer?.style.getPropertyValue("--queue-drawer-bottom")).toBe(
			bottom,
		);
		unmount();
		expect(disconnect).toHaveBeenCalled();
	});

	it("drops the search card over the field again when only the field moves", () => {
		const observed = new Map<Element, () => void>();
		vi.stubGlobal(
			"ResizeObserver",
			class {
				constructor(private callback: () => void) {}
				observe(target: Element) {
					observed.set(target, this.callback);
				}
				unobserve() {}
				disconnect() {}
			},
		);
		let fieldLeft = 300;
		const getBounds = HTMLElement.prototype.getBoundingClientRect;
		vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
			function (this: HTMLElement) {
				if (this.hasAttribute("data-library-search-field"))
					return new DOMRect(fieldLeft, 8, 500, 48);
				if (this.hasAttribute("data-app-shell"))
					return new DOMRect(0, 0, 1600, 900);
				return getBounds.call(this);
			},
		);
		const { container } = renderShell(<AppShell onSearch={vi.fn()} />);
		const shell = container.querySelector<HTMLElement>("[data-app-shell]");
		const field = container.querySelector("[data-library-search-field]");
		expect(shell?.style.getPropertyValue("--library-search-center")).toBe(
			"550px",
		);
		// A smaller Home button beside it: the field moves and grows, the
		// shell does not resize.
		const remeasure = field ? observed.get(field) : undefined;
		if (!remeasure) throw new Error("The search field was not observed");
		fieldLeft = 292;
		act(remeasure);
		expect(shell?.style.getPropertyValue("--library-search-center")).toBe(
			"542px",
		);
	});

	it("drops the search card over the field again when the field moves without resizing", () => {
		const observed = new Map<Element, () => void>();
		vi.stubGlobal(
			"ResizeObserver",
			class {
				constructor(private callback: () => void) {}
				observe(target: Element) {
					observed.set(target, this.callback);
				}
				unobserve() {}
				disconnect() {}
			},
		);
		let fieldLeft = 300;
		const getBounds = HTMLElement.prototype.getBoundingClientRect;
		vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
			function (this: HTMLElement) {
				if (this.hasAttribute("data-library-search-field"))
					return new DOMRect(fieldLeft, 8, 500, 48);
				if (this.hasAttribute("data-app-shell"))
					return new DOMRect(0, 0, 1600, 900);
				return getBounds.call(this);
			},
		);
		const { container } = renderShell(<AppShell onSearch={vi.fn()} />);
		const shell = container.querySelector<HTMLElement>("[data-app-shell]");
		// The drawer's measured width shifts the field left at its own size:
		// only the spacer beside it resizes.
		const spacer = container.querySelector("header")?.children[1];
		const remeasure = spacer ? observed.get(spacer) : undefined;
		if (!remeasure) throw new Error("The Top Nav's children were not observed");
		fieldLeft = 260;
		act(remeasure);
		expect(shell?.style.getPropertyValue("--library-search-center")).toBe(
			"510px",
		);
	});

	it("sizes the search card's input row to end on the top bar's bottom border", () => {
		const observed = new Map<Element, () => void>();
		vi.stubGlobal(
			"ResizeObserver",
			class {
				constructor(private callback: () => void) {}
				observe(target: Element) {
					observed.set(target, this.callback);
				}
				unobserve() {}
				disconnect() {}
			},
		);
		let headerHeight = 64;
		const getBounds = HTMLElement.prototype.getBoundingClientRect;
		vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
			function (this: HTMLElement) {
				if (this.hasAttribute("data-library-search-field"))
					return new DOMRect(300, 8, 500, 48);
				if (this.tagName === "HEADER")
					return new DOMRect(0, 0, 1600, headerHeight);
				if (this.hasAttribute("data-app-shell"))
					return new DOMRect(0, 0, 1600, 900);
				return getBounds.call(this);
			},
		);
		const { container } = renderShell(<AppShell onSearch={vi.fn()} />);
		const shell = container.querySelector<HTMLElement>("[data-app-shell]");
		// From the field's top (8px) to the bar's bottom edge (64px).
		expect(shell?.style.getPropertyValue("--library-search-top")).toBe("8px");
		expect(shell?.style.getPropertyValue("--library-search-row-height")).toBe(
			"56px",
		);
		const remeasure = shell ? observed.get(shell) : undefined;
		if (!remeasure) throw new Error("The shell was not observed");
		headerHeight = 72;
		act(remeasure);
		expect(shell?.style.getPropertyValue("--library-search-row-height")).toBe(
			"64px",
		);
	});

	it("lets the Desktop Client drag the window by the top nav's empty space", () => {
		const { container } = render(<TopNav onSearch={vi.fn()} />);
		const header = container.querySelector("header") as HTMLElement;
		expect(header.hasAttribute("data-window-drag-region")).toBe(true);
		// Links and buttons stay clickable instead of starting a drag.
		for (const control of [
			screen.getByRole("link", { name: "Home" }),
			screen.getByRole("button", { name: "Search your library" }),
		]) {
			expect(control.hasAttribute("data-window-drag-region")).toBe(false);
		}
	});

	it("moves a page's Top Nav actions next to Settings", () => {
		renderShell(
			<AppShell>
				<TopNavActions>
					<button type="button">Filters</button>
				</TopNavActions>
			</AppShell>,
		);
		const filters = screen.getByRole("button", { name: "Filters" });
		expect(filters.closest("header")).toBeTruthy();
		expect(filters.closest("main")).toBeNull();
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

	it("puts Home and the search field in the top bar", () => {
		const onSearch = vi.fn();
		const { container } = render(<TopNav onSearch={onSearch} />);
		expect(
			screen.getByRole("link", { name: "Home" }).getAttribute("href"),
		).toBe("/");
		const field = screen.getByRole("button", { name: "Search your library" });
		expect(field.hasAttribute("data-library-search-field")).toBe(true);
		fireEvent.click(field);
		expect(onSearch).toHaveBeenCalledTimes(1);
		expect(container.querySelector("nav")).toBeNull();
	});

	it("lists the library sections and Settings in the Sidebar", () => {
		render(<Sidebar />);
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
		const radioLink = screen.getByRole("link", { name: "Radio" });
		// The open section's icon carries the accent; its label stays neutral.
		expect(radioLink.className).toContain(
			"[&.active_svg]:text-shell-active-foreground",
		);
		expect(radioLink.className).toContain("[&.active]:text-heading");
		expect(screen.getByRole("link", { name: "Settings" })).toBeTruthy();
		expect(screen.queryByText("Premium Account")).toBeNull();
	});
});
