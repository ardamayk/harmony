import { IPC, type WindowLabel } from "@repo/desktop-bridge";
import { BrowserWindow, shell } from "electron";
import type { PlacementStore } from "./placement-store";
import { rendererWebPreferences, validateExternalUrl } from "./security";

const MAIN_WINDOW = {
	width: 1280,
	height: 800,
	minWidth: 768,
	minHeight: 600,
};
const MINI_WINDOW = { width: 380, height: 104 };

export type WindowManagerOptions = {
	/** Renderer base URL: app://earthly or the Vite dev server origin. */
	rendererUrl: string;
	preload: string;
	icon: string;
	miniPlacement: PlacementStore;
	/** The main window's close button; the core decides hide vs. quit. */
	onMainWindowCloseRequested: () => void;
};

/**
 * The main window and the always-on-top mini player. Both load the same
 * web UI; the mini player is its `/mini` route. They share playback truth
 * through the forwarded `desktop-playback-state` event.
 */
export class WindowManager {
	private main: BrowserWindow | null = null;
	private mini: BrowserWindow | null = null;
	private allowClose = false;

	constructor(private readonly options: WindowManagerOptions) {}

	labelOf(window: BrowserWindow | null): WindowLabel | null {
		if (window === null) return null;
		if (window === this.main) return "main";
		if (window === this.mini) return "mini";
		return null;
	}

	all(): BrowserWindow[] {
		return [this.main, this.mini].filter(
			(window): window is BrowserWindow =>
				window !== null && !window.isDestroyed(),
		);
	}

	createMainWindow(): BrowserWindow {
		const window = new BrowserWindow({
			...MAIN_WINDOW,
			title: "Harmony",
			icon: this.options.icon,
			// The page draws its own frame: rounded corners, drag region and
			// window controls in the Top Nav (see DesktopWindowFrame).
			frame: false,
			transparent: true,
			backgroundColor: "#00000000",
			show: false,
			webPreferences: rendererWebPreferences(this.options.preload),
		});
		this.harden(window);
		window.once("ready-to-show", () => window.show());
		// The window title follows the playing source (shell:window-title),
		// not the page's <title>.
		window.on("page-title-updated", (event) => event.preventDefault());
		window.on("close", (event) => {
			if (this.allowClose) return;
			event.preventDefault();
			this.options.onMainWindowCloseRequested();
		});
		const sendMaximized = (maximized: boolean) =>
			window.webContents.send(IPC.windowMaximized, maximized);
		window.on("maximize", () => sendMaximized(true));
		window.on("unmaximize", () => sendMaximized(false));
		window.on("closed", () => {
			this.main = null;
		});
		void window.loadURL(`${this.options.rendererUrl}/`);
		this.main = window;
		return window;
	}

	showMainWindow(): void {
		const window = this.main ?? this.createMainWindow();
		if (window.isMinimized()) window.restore();
		window.show();
		window.focus();
	}

	hideMainWindow(): void {
		this.main?.hide();
	}

	setMainWindowTitle(title: string): void {
		this.main?.setTitle(title);
	}

	openMiniWindow(): void {
		if (this.mini) {
			this.mini.show();
			this.mini.focus();
			return;
		}
		const placement = this.options.miniPlacement.load();
		const window = new BrowserWindow({
			...MINI_WINDOW,
			...(placement ?? {}),
			title: "Harmony",
			icon: this.options.icon,
			resizable: false,
			maximizable: false,
			fullscreenable: false,
			frame: false,
			alwaysOnTop: true,
			skipTaskbar: true,
			webPreferences: rendererWebPreferences(this.options.preload),
		});
		this.harden(window);
		window.on("page-title-updated", (event) => event.preventDefault());
		window.on("close", () => {
			const [x, y] = window.getPosition();
			try {
				this.options.miniPlacement.save({ x, y });
			} catch (error) {
				console.error("Mini player placement was not saved:", error);
			}
		});
		window.on("closed", () => {
			this.mini = null;
		});
		void window.loadURL(`${this.options.rendererUrl}/mini`);
		this.mini = window;
	}

	closeMiniWindow(): void {
		this.mini?.close();
	}

	toggleMiniWindow(): void {
		if (this.mini) this.closeMiniWindow();
		else this.openMiniWindow();
	}

	/** Lets the windows close for real, once the app is quitting. */
	releaseForQuit(): void {
		this.allowClose = true;
	}

	/**
	 * Keeps each window on the app: no popups, no navigating away. Web links
	 * that try either open in the system browser instead.
	 */
	private harden(window: BrowserWindow) {
		const { rendererUrl } = this.options;
		window.webContents.setWindowOpenHandler(({ url }) => {
			openExternally(url);
			return { action: "deny" };
		});
		window.webContents.on("will-navigate", (event, url) => {
			if (url.startsWith(`${rendererUrl}/`) || url === rendererUrl) return;
			event.preventDefault();
			openExternally(url);
		});
		window.webContents.on("will-attach-webview", (event) =>
			event.preventDefault(),
		);
	}
}

function openExternally(url: string) {
	try {
		void shell.openExternal(validateExternalUrl(url));
	} catch {
		// Not a web URL: drop it.
	}
}
