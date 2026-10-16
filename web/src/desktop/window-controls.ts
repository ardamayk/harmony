import type { WindowControls } from "@repo/ui";
import { desktopWindow } from "./desktop-api";

/**
 * The main window draws no system title bar (see desktop/src/main/windows.ts),
 * so the Top Nav hosts these actions instead.
 */
export const desktopWindowControls: WindowControls = {
	minimize: () => desktopWindow().minimize(),
	toggleMaximize: () => desktopWindow().toggleMaximize(),
	close: () => desktopWindow().close(),
};
