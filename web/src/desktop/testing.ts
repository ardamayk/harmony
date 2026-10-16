// Test double for `window.earthlyDesktop`, standing in for the Electron
// preload in jsdom tests.

import type {
	DesktopCommand,
	DesktopEvent,
	DesktopWindowApi,
	EarthlyDesktopBridge,
} from "@repo/desktop-bridge";

type CommandHandler = (
	command: DesktopCommand,
	args: Record<string, unknown> | undefined,
) => unknown;

export type FakeDesktopBridge = {
	bridge: EarthlyDesktopBridge;
	/** Delivers a core event to every matching listener. */
	emit(event: DesktopEvent, payload: unknown): void;
	listenerCount(event: DesktopEvent): number;
	uninstall(): void;
};

/**
 * Installs a fake bridge on `window`. `handle` answers invokes; a thrown
 * value or rejected promise becomes the command's error.
 */
export function installDesktopBridge(
	handle: CommandHandler = () => null,
	window: Partial<DesktopWindowApi> = {},
): FakeDesktopBridge {
	const listeners = new Map<DesktopEvent, Set<(payload: unknown) => void>>();
	const bridge: EarthlyDesktopBridge = {
		invoke: async <T>(
			command: DesktopCommand,
			args?: Record<string, unknown>,
		) => (await handle(command, args)) as T,
		listen: <T>(event: DesktopEvent, listener: (payload: T) => void) => {
			const set = listeners.get(event) ?? new Set();
			const wrapped = listener as (payload: unknown) => void;
			set.add(wrapped);
			listeners.set(event, set);
			return () => {
				set.delete(wrapped);
			};
		},
		window: {
			label: "main",
			minimize: () => {},
			toggleMaximize: () => {},
			close: () => {},
			isMaximized: async () => false,
			onMaximizedChange: () => () => {},
			...window,
		},
	};
	globalThis.window.earthlyDesktop = bridge;
	return {
		bridge,
		emit(event, payload) {
			for (const listener of listeners.get(event) ?? []) listener(payload);
		},
		listenerCount: (event) => listeners.get(event)?.size ?? 0,
		uninstall() {
			Reflect.deleteProperty(globalThis.window, "earthlyDesktop");
		},
	};
}
