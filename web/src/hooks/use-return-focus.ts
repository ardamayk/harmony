import { useCallback, useRef } from "react";

/**
 * Remembers the element that opened a programmatically controlled dialog so
 * closing the dialog restores keyboard focus to it.
 *
 * A Base UI modal dialog moves focus back to its `DialogTrigger` on close.
 * Dialogs that open from application state (the Import Music action, context
 * menu items whose menu unmounts before the dialog opens) have no trigger, so
 * without this focus falls to `document.body` and keyboard users lose their
 * place.
 */
export function useReturnFocus() {
	const targetRef = useRef<HTMLElement | null>(null);

	/** Record the element to return to; defaults to the focused element. */
	const capture = useCallback((target?: HTMLElement | null) => {
		if (target) {
			targetRef.current = target;
			return;
		}
		const active = document.activeElement;
		targetRef.current =
			active instanceof HTMLElement && active !== document.body ? active : null;
	}, []);

	/**
	 * The Base UI `finalFocus` target: the opener, else `fallback` when the
	 * opener left the DOM, else `true` for the dialog's default.
	 */
	const restore = useCallback(
		(fallback?: HTMLElement | null): HTMLElement | true => {
			const captured = targetRef.current;
			targetRef.current = null;
			const target = captured?.isConnected ? captured : fallback;
			return target?.isConnected ? target : true;
		},
		[],
	);

	return { capture, restore };
}
