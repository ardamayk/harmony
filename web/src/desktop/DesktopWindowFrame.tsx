import { useEffect, useState } from "react";
import { isDesktopClient } from "#/desktop/bridge";
import { desktopWindow } from "#/desktop/desktop-api";

/**
 * The main Desktop Client window draws no system title bar or border (see
 * desktop/src/main/windows.ts), so the page supplies the frame: rounded
 * corners (`data-desktop-window` styles in styles.css) that square off while
 * maximized. Electron resizes frameless windows from their edges itself.
 */
export function DesktopWindowFrame() {
	const [isMainWindow] = useState(
		() => isDesktopClient() && desktopWindow().label === "main",
	);
	const [isMaximized, setIsMaximized] = useState(false);

	useEffect(() => {
		if (!isMainWindow) return;
		const root = document.documentElement;
		root.dataset.desktopWindow = "";
		const appWindow = desktopWindow();
		let cancelled = false;
		void appWindow.isMaximized().then((maximized) => {
			if (!cancelled) setIsMaximized(maximized);
		});
		const stop = appWindow.onMaximizedChange(setIsMaximized);
		return () => {
			cancelled = true;
			delete root.dataset.desktopWindow;
			stop();
		};
	}, [isMainWindow]);

	useEffect(() => {
		if (!isMainWindow) return;
		const root = document.documentElement;
		if (isMaximized) root.dataset.windowMaximized = "";
		else delete root.dataset.windowMaximized;
	}, [isMainWindow, isMaximized]);

	return null;
}
