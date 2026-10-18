import { invoke } from "./desktop-api";

/** Closes the mini player window; a no-op when it is not open. */
export function closeMiniPlayer(): Promise<void> {
	return invoke("desktop_close_mini_player");
}

export function toggleMiniPlayer(): Promise<void> {
	return invoke("desktop_toggle_mini_player");
}

/** Shows and focuses the main window, for the mini player's expand button. */
export function showMainWindow(): Promise<void> {
	return invoke("desktop_show_main_window");
}
