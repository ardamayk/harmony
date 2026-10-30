import { vi } from "vite-plus/test";

type Scroll = "in view" | "scrolled past" | "below the fold";

const ROOT_TOP = 64;

/**
 * Replaces IntersectionObserver for a test: `scroll` moves the observed
 * element as seen from a scroller whose top edge is under the Top Nav.
 */
export function stubIntersectionObserver() {
	let notify: (
		entries: Partial<IntersectionObserverEntry>[],
	) => void = () => {};
	const options: Array<IntersectionObserverInit | undefined> = [];
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			constructor(callback: typeof notify, init?: IntersectionObserverInit) {
				notify = callback;
				options.push(init);
			}
			observe() {}
			disconnect() {}
		},
	);
	return {
		options,
		scroll(to: Scroll) {
			notify([
				{
					isIntersecting: to === "in view",
					boundingClientRect: {
						top: to === "below the fold" ? 900 : ROOT_TOP - 100,
					} as DOMRectReadOnly,
					rootBounds: { top: ROOT_TOP } as DOMRectReadOnly,
				},
			]);
		},
	};
}
