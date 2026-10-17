const TEXT_ENTRY_SELECTOR = "input, textarea, select, [contenteditable]";

/**
 * Chromium turns on `:focus-visible` for any key press, Escape included, so
 * a control focused by a click shows its keyboard ring after Escape. While
 * the user is on the pointer, Escape keeps the ring hidden, also on the
 * opener a closing dialog returns focus to.
 */
export function suppressEscapeFocusRing(doc: Document = document) {
	let usingPointer = false;
	let escapedWithPointer = false;

	const hideRing = (element: EventTarget | null) => {
		if (!(element instanceof HTMLElement)) return;
		if (!element.matches(":focus-visible")) return;
		if (element.matches(TEXT_ENTRY_SELECTOR)) return;
		element.blur();
		element.focus({ preventScroll: true, focusVisible: false });
	};

	const onPointerDown = () => {
		usingPointer = true;
		escapedWithPointer = false;
	};
	const onKeyDown = (event: KeyboardEvent) => {
		if (event.key !== "Escape") {
			usingPointer = false;
			escapedWithPointer = false;
			return;
		}
		if (!usingPointer) return;
		escapedWithPointer = true;
		// Chromium flips `:focus-visible` after this listener runs.
		setTimeout(() => hideRing(doc.activeElement));
	};
	const onFocusIn = (event: FocusEvent) => {
		if (escapedWithPointer) hideRing(event.target);
	};

	doc.addEventListener("pointerdown", onPointerDown, true);
	doc.addEventListener("keydown", onKeyDown, true);
	doc.addEventListener("focusin", onFocusIn, true);
	return () => {
		doc.removeEventListener("pointerdown", onPointerDown, true);
		doc.removeEventListener("keydown", onKeyDown, true);
		doc.removeEventListener("focusin", onFocusIn, true);
	};
}
