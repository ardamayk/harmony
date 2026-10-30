// Electron main process of the Harmony Desktop Client. It owns the
// windows, tray, native pickers and the app's lifetime, and starts the Rust
// core (native playback, media proxy, Managed Import) as a child process.
// See docs/adr/0024-electron-shell-with-rust-core.md.

import { DESKTOP_EVENTS, IPC } from "@repo/desktop-bridge";
import {
	app,
	BrowserWindow,
	dialog,
	type IpcMainEvent,
	type IpcMainInvokeEvent,
	ipcMain,
	net,
	protocol,
	shell,
} from "electron";
import { join } from "node:path";
import { handleInvoke, IMPORT_EXTENSIONS, runWindowAction } from "./commands";
import { CoreProcess } from "./core-process";
import { devServerOrigin, shellPaths } from "./paths";
import { PlacementStore } from "./placement-store";
import { createRendererHandler } from "./renderer-protocol";
import {
	isTrustedRendererUrl,
	RENDERER_ORIGIN,
	RENDERER_SCHEME,
} from "./security";
import { PlaybackTray, type TrayAction, type TrayUpdate } from "./tray";
import { WindowManager } from "./windows";

/** Settings directory shared with the core (and the former Tauri client). */
const CONFIG_DIRECTORY_NAME = "audio.earthly.desktop";
const FORWARDED_EVENTS = new Set<string>(Object.values(DESKTOP_EVENTS));

app.setName("Harmony");
protocol.registerSchemesAsPrivileged([
	{
		scheme: RENDERER_SCHEME,
		privileges: {
			standard: true,
			secure: true,
			supportFetchAPI: true,
			codeCache: true,
		},
	},
]);

if (!app.requestSingleInstanceLock()) {
	// The running instance owns the media proxy port; it shows its window.
	app.exit(0);
} else {
	void start();
}

async function start() {
	const desktopDirectory = join(__dirname, "..");
	const paths = shellPaths(desktopDirectory, process.env);
	const devOrigin = devServerOrigin(process.env);
	const rendererUrl = devOrigin ?? RENDERER_ORIGIN;
	const configDirectory = join(app.getPath("appData"), CONFIG_DIRECTORY_NAME);

	await app.whenReady();

	const core = new CoreProcess({
		executable: paths.core,
		args: ["--config-dir", configDirectory],
		env: process.env,
	});
	let quitting = false;

	const sendShellAction = (action: TrayAction | "close-main-window") => {
		core.request("shell_action", { action }).catch((error: unknown) => {
			console.error(`Shell action '${action}' failed:`, error);
		});
	};

	const windows = new WindowManager({
		rendererUrl,
		preload: paths.preload,
		icon: paths.icon,
		miniPlacement: new PlacementStore(
			join(configDirectory, "mini-window.json"),
		),
		onMainWindowCloseRequested: () => sendShellAction("close-main-window"),
	});
	let tray: PlaybackTray | null = null;
	// The core sends its first tray update before shell:ready; replay it so
	// the tray has its menu (Open, Quit) even if no playback update follows.
	let pendingTrayUpdate: TrayUpdate | null = null;

	const quit = () => {
		if (quitting) return;
		quitting = true;
		windows.releaseForQuit();
		void core.stop().finally(() => {
			tray?.destroy();
			app.exit(0);
		});
	};

	core.on("event", (name, payload) => {
		switch (name) {
			case "shell:ready":
				tray = new PlaybackTray(paths.trayIcon, sendShellAction);
				if (pendingTrayUpdate) tray.update(pendingTrayUpdate);
				windows.createMainWindow();
				return;
			case "shell:show-main-window":
				return windows.showMainWindow();
			case "shell:hide-main-window":
				return windows.hideMainWindow();
			case "shell:toggle-mini-window":
				return windows.toggleMiniWindow();
			case "shell:window-title":
				return windows.setMainWindowTitle((payload as { title: string }).title);
			case "shell:tray":
				if (tray) return tray.update(payload as TrayUpdate);
				pendingTrayUpdate = payload as TrayUpdate;
				return;
			case "shell:exit":
				return quit();
		}
		if (!FORWARDED_EVENTS.has(name)) return;
		for (const window of windows.all()) {
			window.webContents.send(IPC.event, name, payload);
		}
	});

	core.on("exit", (code, signal) => {
		if (quitting) return;
		quitting = true;
		dialog.showErrorBox(
			"Harmony stopped",
			`The Desktop core exited (${signal ?? `code ${code}`}).\n\n${core.diagnostics}`,
		);
		app.exit(1);
	});

	// Quitting from outside the tray (session end, Ctrl+C in development)
	// still goes through the core so the Playback Session is saved.
	app.on("before-quit", (event) => {
		if (quitting) return;
		event.preventDefault();
		quit();
	});
	app.on("second-instance", () => windows.showMainWindow());
	// The app keeps running in the tray with every window closed.
	app.on("window-all-closed", () => {});

	if (devOrigin === null) {
		protocol.handle(
			RENDERER_SCHEME,
			createRendererHandler(paths.rendererRoot, (url) => net.fetch(url)),
		);
	}

	const isTrusted = (event: IpcMainEvent | IpcMainInvokeEvent) =>
		isTrustedRendererUrl(event.senderFrame?.url ?? "", devOrigin);
	const windowOf = (event: IpcMainEvent | IpcMainInvokeEvent) =>
		BrowserWindow.fromWebContents(event.sender);

	ipcMain.handle(IPC.invoke, (event, command: unknown, args: unknown) => {
		if (!isTrusted(event)) {
			return { ok: false, error: "Untrusted renderer." };
		}
		return handleInvoke(
			{
				requestCore: (method, params) => core.request(method, params),
				pickImportFiles: async () => {
					const result = await dialog.showOpenDialog({
						properties: ["openFile", "multiSelections"],
						filters: [{ name: "Audio", extensions: IMPORT_EXTENSIONS }],
					});
					return result.canceled ? [] : result.filePaths;
				},
				pickImportFolder: async () => {
					const result = await dialog.showOpenDialog({
						properties: ["openDirectory"],
					});
					return result.canceled ? [] : result.filePaths;
				},
				openExternal: (url) => shell.openExternal(url),
				openMiniWindow: () => windows.openMiniWindow(),
				closeMiniWindow: () => windows.closeMiniWindow(),
				toggleMiniWindow: () => windows.toggleMiniWindow(),
				showMainWindow: () => windows.showMainWindow(),
			},
			command,
			args,
		);
	});

	ipcMain.on(IPC.windowLabel, (event) => {
		event.returnValue = isTrusted(event)
			? windows.labelOf(windowOf(event))
			: null;
	});

	ipcMain.handle(IPC.windowIsMaximized, (event) =>
		isTrusted(event) ? (windowOf(event)?.isMaximized() ?? false) : false,
	);

	ipcMain.on(IPC.window, (event, action: unknown) => {
		const window = windowOf(event);
		if (!isTrusted(event) || window === null) return;
		runWindowAction(window, action);
	});
}
