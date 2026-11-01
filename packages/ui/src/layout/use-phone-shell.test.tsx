import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { PHONE_SHELL_QUERY, usePhoneShell } from "./use-phone-shell";

function stubMatchMedia(matches: boolean) {
	const listeners = new Set<() => void>();
	const media = {
		matches,
		addEventListener: (_: string, listener: () => void) =>
			listeners.add(listener),
		removeEventListener: (_: string, listener: () => void) =>
			listeners.delete(listener),
	};
	const matchMedia = vi.fn((_query: string) => media);
	vi.stubGlobal("matchMedia", matchMedia);
	return { media, listeners, matchMedia };
}

describe("usePhoneShell", () => {
	afterEach(() => {
		cleanup();
		vi.unstubAllGlobals();
	});

	it("is false without matchMedia", () => {
		vi.stubGlobal("matchMedia", undefined);
		expect(renderHook(() => usePhoneShell()).result.current).toBe(false);
	});

	it("is right on the first render", () => {
		const { matchMedia } = stubMatchMedia(true);
		const seen: boolean[] = [];
		renderHook(() => seen.push(usePhoneShell()));
		expect(seen[0]).toBe(true);
		expect(matchMedia).toHaveBeenCalledWith(PHONE_SHELL_QUERY);
	});

	it("is false in the Desktop Client, which zoom can take below 768px", () => {
		stubMatchMedia(true);
		document.documentElement.dataset.desktopClient = "";
		const { result } = renderHook(() => usePhoneShell());
		delete document.documentElement.dataset.desktopClient;
		expect(result.current).toBe(false);
	});

	it("follows the query and stops listening on unmount", () => {
		const { media, listeners } = stubMatchMedia(false);
		const { result, unmount } = renderHook(() => usePhoneShell());
		expect(result.current).toBe(false);
		act(() => {
			media.matches = true;
			for (const listener of listeners) listener();
		});
		expect(result.current).toBe(true);
		unmount();
		expect(listeners.size).toBe(0);
	});
});
