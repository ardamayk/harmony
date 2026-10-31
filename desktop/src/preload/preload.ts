// Sandboxed preload: the renderer's only door to the Desktop Client. It
// exposes `window.earthlyDesktop` (see @repo/desktop-bridge) and nothing of
// Node.js or Electron itself.

import {
	DESKTOP_BRIDGE_KEY,
	type DesktopEvent,
	type EarthlyDesktopBridge,
	IPC,
	type InvokeResult,
	type WindowAction,
} from "@repo/desktop-bridge";
import { contextBridge, type IpcRendererEvent, ipcRenderer } from "electron";

const sendWindowAction = (action: WindowAction) =>
	ipcRenderer.send(IPC.window, action);

const bridge: EarthlyDesktopBridge = {
	async invoke<T>(command: string, args?: Record<string, unknown>) {
		const result = (await ipcRenderer.invoke(
			IPC.invoke,
			command,
			args,
		)) as InvokeResult;
		if (!result.ok) throw result.error;
		return result.value as T;
	},
	listen<T>(event: DesktopEvent, listener: (payload: T) => void) {
		const handler = (_: IpcRendererEvent, name: string, payload: T) => {
			if (name === event) listener(payload);
		};
		ipcRenderer.on(IPC.event, handler);
		return () => {
			ipcRenderer.removeListener(IPC.event, handler);
		};
	},
	window: {
		minimize: () => sendWindowAction("minimize"),
		toggleMaximize: () => sendWindowAction("toggle-maximize"),
		close: () => sendWindowAction("close"),
		isMaximized: () =>
			ipcRenderer.invoke(IPC.windowIsMaximized) as Promise<boolean>,
		onMaximizedChange(listener) {
			const handler = (_: IpcRendererEvent, maximized: boolean) =>
				listener(maximized);
			ipcRenderer.on(IPC.windowMaximized, handler);
			return () => {
				ipcRenderer.removeListener(IPC.windowMaximized, handler);
			};
		},
	},
};

contextBridge.exposeInMainWorld(DESKTOP_BRIDGE_KEY, bridge);
