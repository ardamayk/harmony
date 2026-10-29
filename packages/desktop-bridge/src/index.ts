// The contract between the Desktop Client's renderer (web/) and the Electron
// shell (desktop/): what the preload script exposes as `window.earthlyDesktop`,
// the IPC channels behind it, and which commands renderer content may send.
// The runtime schemas the main process parses each message with live in
// `@repo/desktop-bridge/schemas`.

import type { DesktopCommandArgsMap } from "./schemas";

/** Name of the preload API on `window`. */
export const DESKTOP_BRIDGE_KEY = "earthlyDesktop";

/** IPC channels between the preload script and the main process. */
export const IPC = {
	/** renderer → main: `(command, args)`, answered with an {@link InvokeResult}. */
	invoke: "earthly:invoke",
	/** main → renderer: `(event, payload)` for every forwarded core event. */
	event: "earthly:event",
	/** renderer → main: one {@link WindowAction} for the sender's window. */
	window: "earthly:window",
	/** renderer → main (sync): the sender window's {@link WindowLabel}. */
	windowLabel: "earthly:window-label",
	/** renderer → main: whether the sender's window is maximized. */
	windowIsMaximized: "earthly:window-is-maximized",
	/** main → renderer: the window was maximized (`true`) or restored. */
	windowMaximized: "earthly:window-maximized",
} as const;

export type WindowLabel = "main" | "mini";

export type WindowAction = "minimize" | "toggle-maximize" | "close";

/**
 * Commands the renderer may send that the Rust core answers. Anything not
 * listed here or in {@link SHELL_COMMANDS} is refused by the main process,
 * so renderer content can never register file paths or drive the shell.
 */
export const CORE_COMMANDS = [
	"get_server_connection",
	"test_server_connection",
	"save_server_connection",
	"desktop_http_request",
	// Ends the in-flight `desktop_http_request` sent with the same `requestId`.
	"desktop_cancel_http_request",
	"desktop_upload_import_file",
	"desktop_cancel_import_upload",
	"desktop_release_import_selections",
	"get_media_proxy_url",
	"desktop_reconnect_queue_events",
	"get_desktop_playback_state",
	"get_desktop_mpv_status",
	"desktop_playback_renderer_ready",
	"desktop_playback_quit",
	"desktop_playback_play",
	"desktop_playback_pause",
	"desktop_playback_stop",
	"desktop_playback_toggle_play",
	"desktop_playback_sync_queue_context",
	"desktop_playback_previous",
	"desktop_playback_next",
	"desktop_playback_seek",
	"desktop_playback_set_volume",
	"desktop_playback_set_playback_rate",
	"desktop_playback_set_stop_after_current",
	"desktop_playback_set_transition_fade",
	"desktop_playback_set_processing_profile",
	"desktop_playback_set_replay_gain",
	"desktop_playback_set_equalizer_preset",
	"desktop_playback_set_equalizer_gain",
	"desktop_playback_refresh_output_devices",
	"desktop_playback_select_direct_alsa_output",
	"desktop_playback_select_exclusive_output",
	"desktop_playback_fallback_to_system_output",
	"desktop_playback_enable_adaptive_system_rate",
	"desktop_playback_toggle_shuffle",
	"desktop_playback_cycle_repeat_mode",
] as const;

/** Commands the main process answers itself: windows, pickers, links. */
export const SHELL_COMMANDS = [
	"desktop_select_import_files",
	"desktop_select_import_folder",
	"desktop_open_external_url",
	"desktop_open_mini_player",
	"desktop_close_mini_player",
	"desktop_toggle_mini_player",
	"desktop_show_main_window",
] as const;

export type CoreCommand = (typeof CORE_COMMANDS)[number];
export type ShellCommand = (typeof SHELL_COMMANDS)[number];
export type DesktopCommand = CoreCommand | ShellCommand;

/** The arguments a command accepts (see DESKTOP_COMMAND_ARGS). */
export type DesktopCommandArgs<C extends DesktopCommand = DesktopCommand> =
	DesktopCommandArgsMap[C];

/** Core events the main process forwards to every renderer window. */
export const DESKTOP_EVENTS = {
	playbackState: "desktop-playback-state",
	connectionChanged: "server-connection-changed",
	queueInvalidated: "desktop-queue-invalidated",
	queueEventsError: "desktop-queue-events-error",
	importProgress: "desktop-import-progress",
} as const;

export type DesktopEvent = (typeof DESKTOP_EVENTS)[keyof typeof DESKTOP_EVENTS];

export type ImportProgressEvent = {
	uploadId: string;
	sentBytes: number;
	totalBytes: number;
};

/**
 * How the main process answers an invoke. The preload script unwraps it so
 * a failed command rejects with the command's own error value (for example
 * `{ code, message }`), not an Electron-wrapped `Error`.
 */
export type InvokeResult =
	| { ok: true; value: unknown }
	| { ok: false; error: unknown };

export type Unlisten = () => void;

export type DesktopWindowApi = {
	readonly label: WindowLabel;
	minimize(): void;
	toggleMaximize(): void;
	close(): void;
	isMaximized(): Promise<boolean>;
	onMaximizedChange(listener: (maximized: boolean) => void): Unlisten;
};

/** `window.earthlyDesktop`, present only inside the Desktop Client. */
export type EarthlyDesktopBridge = {
	invoke<T = unknown, C extends DesktopCommand = DesktopCommand>(
		command: C,
		args?: DesktopCommandArgs<C>,
	): Promise<T>;
	listen<T = unknown>(
		event: DesktopEvent,
		listener: (payload: T) => void,
	): Unlisten;
	readonly window: DesktopWindowApi;
};

export function isCoreCommand(command: string): command is CoreCommand {
	return (CORE_COMMANDS as readonly string[]).includes(command);
}
