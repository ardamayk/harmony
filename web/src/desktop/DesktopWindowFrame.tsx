import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { isDesktopClient } from "#/desktop/bridge";

const MAIN_WINDOW_LABEL = "main";

type ResizeDirection = Parameters<
	ReturnType<typeof getCurrentWindow>["startResizeDragging"]
>[0];

const RESIZE_HANDLES: Array<{ direction: ResizeDirection; className: string }> =
	[
		{ direction: "North", className: "inset-x-3 top-0 h-1.5 cursor-ns-resize" },
		{
			direction: "South",
			className: "inset-x-3 bottom-0 h-1.5 cursor-ns-resize",
		},
		{ direction: "West", className: "inset-y-3 left-0 w-1.5 cursor-ew-resize" },
		{
			direction: "East",
			className: "inset-y-3 right-0 w-1.5 cursor-ew-resize",
		},
		{
			direction: "NorthWest",
			className: "top-0 left-0 size-3 cursor-nwse-resize",
		},
		{
			direction: "NorthEast",
			className: "top-0 right-0 size-3 cursor-nesw-resize",
		},
		{
			direction: "SouthWest",
			className: "bottom-0 left-0 size-3 cursor-nesw-resize",
		},
		{
			direction: "SouthEast",
			className: "right-0 bottom-0 size-3 cursor-nwse-resize",
		},
	];

/**
 * The main Desktop Client window draws no system title bar or border (see
 * tauri.conf.json), so this frame supplies what the window manager no longer
 * does: rounded corners (`data-desktop-window` styles in styles.css) and
 * edge handles to resize the window. Both step aside while maximized.
 */
export function DesktopWindowFrame() {
	const [isMainWindow] = useState(
		() => isDesktopClient() && getCurrentWindow().label === MAIN_WINDOW_LABEL,
	);
	const [isMaximized, setIsMaximized] = useState(false);

	useEffect(() => {
		if (!isMainWindow) return;
		const root = document.documentElement;
		root.dataset.desktopWindow = "";
		const appWindow = getCurrentWindow();
		let cancelled = false;
		const sync = () =>
			void appWindow.isMaximized().then((maximized) => {
				if (!cancelled) setIsMaximized(maximized);
			});
		sync();
		const unlisten = appWindow.onResized(sync);
		return () => {
			cancelled = true;
			delete root.dataset.desktopWindow;
			void unlisten.then((stop) => stop());
		};
	}, [isMainWindow]);

	useEffect(() => {
		if (!isMainWindow) return;
		const root = document.documentElement;
		if (isMaximized) root.dataset.windowMaximized = "";
		else delete root.dataset.windowMaximized;
	}, [isMainWindow, isMaximized]);

	if (!isMainWindow || isMaximized) return null;
	return (
		<div aria-hidden className="pointer-events-none fixed inset-0 z-[200]">
			{RESIZE_HANDLES.map(({ direction, className }) => (
				<div
					key={direction}
					role="presentation"
					data-window-resize={direction}
					className={`pointer-events-auto absolute ${className}`}
					onMouseDown={(event) => {
						if (event.button !== 0) return;
						event.preventDefault();
						void getCurrentWindow().startResizeDragging(direction);
					}}
				/>
			))}
		</div>
	);
}
