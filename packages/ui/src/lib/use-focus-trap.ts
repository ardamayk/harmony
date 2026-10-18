import { type RefObject, useEffect } from "react";

const FOCUSABLE_SELECTOR = [
	"a[href]",
	"button:not([disabled])",
	"input:not([disabled]):not([type='hidden'])",
	"select:not([disabled])",
	"textarea:not([disabled])",
	"[tabindex]:not([tabindex='-1'])",
].join(",");

export function focusableElements(container: HTMLElement): HTMLElement[] {
	return Array.from(
		container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
	).filter((element) => !element.hasAttribute("aria-hidden"));
}

/**
 * Keeps keyboard focus inside `container` while `active`: moves focus in on
 * activation, cycles Tab and Shift+Tab at the edges, and returns focus to
 * the element that had it before the overlay opened.
 */
export function useFocusTrap(
	container: RefObject<HTMLElement | null>,
	active = true,
) {
	useEffect(() => {
		if (!active) return undefined;
		const element = container.current;
		if (!element) return undefined;
		const previouslyFocused =
			document.activeElement instanceof HTMLElement
				? document.activeElement
				: null;

		const focusables = focusableElements(element);
		(focusables[0] ?? element).focus({ preventScroll: true });

		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key !== "Tab") return;
			const items = focusableElements(element);
			if (items.length === 0) {
				event.preventDefault();
				element.focus();
				return;
			}
			const first = items[0];
			const last = items[items.length - 1];
			const current = document.activeElement;
			if (event.shiftKey && (current === first || !element.contains(current))) {
				event.preventDefault();
				last.focus();
			} else if (!event.shiftKey && current === last) {
				event.preventDefault();
				first.focus();
			}
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => {
			document.removeEventListener("keydown", handleKeyDown);
			if (previouslyFocused?.isConnected) {
				previouslyFocused.focus({ preventScroll: true });
			}
		};
	}, [active, container]);
}
