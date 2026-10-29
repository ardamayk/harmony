import {
	DESKTOP_BRIDGE_KEY,
	type DesktopCommand,
	type DesktopCommandArgs,
	type DesktopEvent,
	type EarthlyDesktopBridge,
	type Unlisten,
} from "@repo/desktop-bridge";

declare global {
	interface Window {
		/** Set by the Desktop Client's preload script; absent in browsers. */
		[DESKTOP_BRIDGE_KEY]?: EarthlyDesktopBridge;
	}
}

export type { Unlisten };

export function hasDesktopBridge(): boolean {
	return typeof window !== "undefined" && window[DESKTOP_BRIDGE_KEY] != null;
}

function desktopBridge(): EarthlyDesktopBridge {
	const bridge =
		typeof window === "undefined" ? undefined : window[DESKTOP_BRIDGE_KEY];
	if (!bridge) throw new Error("The Desktop Client bridge is unavailable.");
	return bridge;
}

/**
 * Sends a command to the Desktop Client. A failed command rejects with the
 * command's own error value, as the Rust core or the shell produced it.
 */
export function invoke<T, C extends DesktopCommand = DesktopCommand>(
	command: C,
	args?: DesktopCommandArgs<C>,
): Promise<T> {
	try {
		return desktopBridge().invoke<T, C>(command, args);
	} catch (error) {
		return Promise.reject(error);
	}
}

export function listen<T>(
	event: DesktopEvent,
	listener: (payload: T) => void,
): Unlisten {
	return desktopBridge().listen<T>(event, listener);
}

/** The window this renderer runs in. */
export function desktopWindow(): EarthlyDesktopBridge["window"] {
	return desktopBridge().window;
}
