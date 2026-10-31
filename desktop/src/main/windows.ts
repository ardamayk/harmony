import { IPC } from "@repo/desktop-bridge";
import { BrowserWindow, shell } from "electron";
import { rendererWebPreferences, validateExternalUrl } from "./security";

const MAIN_WINDOW = {
	width: 1280,
	height: 800,
	minWidth: 768,
	minHeight: 600,
};

export type WindowManagerOptions = {
	/** Renderer base URL: app://earthly or the Vite dev server origin. */
	rendererUrl: string;
	preload: string;
	icon: string;
	/** The main window's close button; the core decides hide vs. quit. */
	onMainWindowCloseRequested: () => void;
};

const WM_SYSCOMMAND = 0x0112;
// SC_MAXIMIZE and SC_RESTORE with HTCAPTION: what a double click on the title
// bar sends.
const MAXIMIZE_FROM_CAPTION = 0xf032;
const RESTORE_FROM_CAPTION = 0xf122;

/**
 * Windows asks a window to maximize on a double click on its title bar, which
 * here is a drag region, but a transparent window ignores the request. Electron
 * maximizes such a window itself, so Windows may ask to maximize one that
 * already is: the request toggles.
 */
function followCaptionDoubleClick(window: BrowserWindow) {
	window.hookWindowMessage(WM_SYSCOMMAND, (wParam) => {
		const command = wParam.readUInt32LE(0);
		if (command !== MAXIMIZE_FROM_CAPTION && command !== RESTORE_FROM_CAPTION)
			return;
		if (window.isMaximized()) window.unmaximize();
		else window.maximize();
	});
}

export class WindowManager {
	private main: BrowserWindow | null = null;
	private allowClose = false;

	constructor(private readonly options: WindowManagerOptions) {}

	/** Hands a core event to the page; dropped while the window is closed. */
	forwardEvent(name: string, payload: unknown): void {
		if (this.main?.isDestroyed() === false)
			this.main.webContents.send(IPC.event, name, payload);
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
		if (process.platform === "win32") followCaptionDoubleClick(window);
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

	/** Lets the window close for real, once the app is quitting. */
	releaseForQuit(): void {
		this.allowClose = true;
	}

	/**
	 * Keeps the window on the app: no popups, no navigating away. Web links
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
