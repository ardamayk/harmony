import { useSyncExternalStore } from "react";

/** Mirrored by the `phone` variant in the Web Client's styles.css. */
export const PHONE_SHELL_QUERY =
	"(max-width: 767px), (pointer: coarse) and (max-height: 500px)";

function subscribe(onChange: () => void): () => void {
	if (typeof window.matchMedia !== "function") return () => {};
	const media = window.matchMedia(PHONE_SHELL_QUERY);
	media.addEventListener("change", onChange);
	return () => media.removeEventListener("change", onChange);
}

function matchesPhoneShell(): boolean {
	return (
		typeof window.matchMedia === "function" &&
		window.matchMedia(PHONE_SHELL_QUERY).matches
	);
}

/** Whether this window gets the phone shell; right from the first render (ADR 0041). */
export function usePhoneShell(): boolean {
	return useSyncExternalStore(subscribe, matchesPhoneShell, () => false);
}
