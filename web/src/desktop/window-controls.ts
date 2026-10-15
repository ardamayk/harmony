import { getCurrentWindow } from "@tauri-apps/api/window";
import type { WindowControls } from "@repo/ui";

/**
 * The main window draws no system title bar (see tauri.conf.json), so the
 * Top Nav hosts these actions instead.
 */
export const desktopWindowControls: WindowControls = {
	minimize: () => void getCurrentWindow().minimize(),
	toggleMaximize: () => void getCurrentWindow().toggleMaximize(),
	close: () => void getCurrentWindow().close(),
};
