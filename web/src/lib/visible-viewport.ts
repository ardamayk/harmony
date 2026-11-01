/**
 * Keeps a fixed element over what the on-screen keyboard leaves. The keyboard
 * shrinks only the visual viewport, so `100dvh` would run under it.
 */
export function fitToVisibleViewport(element: HTMLElement): () => void {
	const viewport = window.visualViewport;
	const fit = () => {
		element.style.top = `${viewport?.offsetTop ?? 0}px`;
		element.style.height = `${viewport?.height ?? window.innerHeight}px`;
	};
	fit();
	viewport?.addEventListener("resize", fit);
	viewport?.addEventListener("scroll", fit);
	return () => {
		viewport?.removeEventListener("resize", fit);
		viewport?.removeEventListener("scroll", fit);
	};
}
