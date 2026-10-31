import { useEffect } from "react";

export interface PlaybackKeyboardActions {
	togglePlay: () => void;
	navigatePrevious: () => void;
	navigateNext: () => void;
	/** Seek relative to the current position; ignored while nothing seekable plays. */
	seekBy?: (deltaSeconds: number) => void;
	/** Change volume by a signed fraction of full scale. */
	adjustVolume?: (delta: number) => void;
	toggleMute?: () => void;
	toggleLyrics?: () => void;
	toggleQueue?: () => void;
	toggleHelp?: () => void;
}

export interface PlaybackKeyboardOptions {
	seekStepSeconds?: number;
	seekStepLargeSeconds?: number;
	volumeStep?: number;
}

const DEFAULT_SEEK_STEP_SECONDS = 5;
const DEFAULT_SEEK_STEP_LARGE_SECONDS = 30;
const DEFAULT_VOLUME_STEP = 0.05;

type PlaybackShortcutDescription = {
	keys: string[];
	description: string;
};

/** Human-readable binding list, the single source for the help overlay. */
export function describePlaybackShortcuts({
	seekStepSeconds = DEFAULT_SEEK_STEP_SECONDS,
	seekStepLargeSeconds = DEFAULT_SEEK_STEP_LARGE_SECONDS,
}: PlaybackKeyboardOptions = {}): PlaybackShortcutDescription[] {
	return [
		{ keys: ["Space"], description: "Play or pause" },
		{ keys: ["N"], description: "Next track" },
		{ keys: ["P"], description: "Previous track" },
		{ keys: ["←", "→"], description: `Seek ${seekStepSeconds} seconds` },
		{
			keys: ["Shift", "←", "→"],
			description: `Seek ${seekStepLargeSeconds} seconds`,
		},
		{ keys: ["↑", "↓"], description: "Volume up or down" },
		{ keys: ["M"], description: "Mute or unmute" },
		{ keys: ["L"], description: "Lyrics" },
		{ keys: ["Q"], description: "Queue panel" },
		{ keys: ["?"], description: "Keyboard shortcuts" },
	];
}

const EDITABLE_INPUT_TYPES = new Set([
	"text",
	"search",
	"email",
	"url",
	"password",
	"number",
	"tel",
]);

/**
 * Shortcuts must not steal keystrokes from places where the user is typing or
 * where the browser already gives the key a meaning (buttons, links, sliders,
 * checkboxes). Those controls keep their native behaviour.
 */
export function shouldIgnorePlaybackShortcut(target: EventTarget | null) {
	if (!(target instanceof HTMLElement)) return false;
	if (target.isContentEditable) return true;
	if (target instanceof HTMLTextAreaElement) return true;
	if (target instanceof HTMLSelectElement) return true;
	if (target instanceof HTMLInputElement) {
		return EDITABLE_INPUT_TYPES.has(target.type) || target.type === "range";
	}
	if (
		target instanceof HTMLButtonElement ||
		target instanceof HTMLAnchorElement
	) {
		return true;
	}
	const role = target.getAttribute("role");
	return (
		role === "button" ||
		role === "slider" ||
		role === "checkbox" ||
		role === "switch" ||
		role === "menuitem" ||
		role === "option" ||
		role === "textbox"
	);
}

/**
 * Global playback shortcuts. Mount once, next to the player bar. Letter and
 * arrow keys are only claimed when no modifier other than Shift is held, so
 * browser and OS chords keep working.
 */
export function usePlaybackKeyboardShortcuts(
	{
		togglePlay,
		navigatePrevious,
		navigateNext,
		seekBy,
		adjustVolume,
		toggleMute,
		toggleLyrics,
		toggleQueue,
		toggleHelp,
	}: PlaybackKeyboardActions,
	{
		seekStepSeconds = DEFAULT_SEEK_STEP_SECONDS,
		seekStepLargeSeconds = DEFAULT_SEEK_STEP_LARGE_SECONDS,
		volumeStep = DEFAULT_VOLUME_STEP,
	}: PlaybackKeyboardOptions = {},
) {
	useEffect(() => {
		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.defaultPrevented) return;
			switch (event.key) {
				case "MediaPlayPause":
					event.preventDefault();
					togglePlay();
					return;
				case "MediaTrackNext":
					event.preventDefault();
					navigateNext();
					return;
				case "MediaTrackPrevious":
					event.preventDefault();
					navigatePrevious();
					return;
				default:
					break;
			}
			if (event.ctrlKey || event.metaKey || event.altKey) return;
			if (shouldIgnorePlaybackShortcut(event.target)) return;
			const seekStep = event.shiftKey ? seekStepLargeSeconds : seekStepSeconds;
			const claim = (action: (() => void) | undefined) => {
				if (!action) return;
				event.preventDefault();
				action();
			};
			switch (event.key) {
				case " ":
					if (event.repeat) return;
					claim(togglePlay);
					return;
				case "ArrowRight":
					claim(seekBy && (() => seekBy(seekStep)));
					return;
				case "ArrowLeft":
					claim(seekBy && (() => seekBy(-seekStep)));
					return;
				case "ArrowUp":
					claim(adjustVolume && (() => adjustVolume(volumeStep)));
					return;
				case "ArrowDown":
					claim(adjustVolume && (() => adjustVolume(-volumeStep)));
					return;
				default:
					break;
			}
			if (event.repeat) return;
			switch (event.key.toLowerCase()) {
				case "n":
					claim(navigateNext);
					return;
				case "p":
					claim(navigatePrevious);
					return;
				case "m":
					claim(toggleMute);
					return;
				case "l":
					claim(toggleLyrics);
					return;
				case "q":
					claim(toggleQueue);
					return;
				case "?":
					claim(toggleHelp);
					return;
				default:
					return;
			}
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [
		togglePlay,
		navigatePrevious,
		navigateNext,
		seekBy,
		adjustVolume,
		toggleMute,
		toggleLyrics,
		toggleQueue,
		toggleHelp,
		seekStepSeconds,
		seekStepLargeSeconds,
		volumeStep,
	]);
}
