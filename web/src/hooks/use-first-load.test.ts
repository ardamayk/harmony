import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vite-plus/test";
import { useFirstLoad } from "./use-first-load";

describe("useFirstLoad", () => {
	it("holds until loading first ends", () => {
		const { result, rerender } = renderHook(
			({ isLoading }) => useFirstLoad(isLoading),
			{ initialProps: { isLoading: true } },
		);
		expect(result.current).toBe(true);

		rerender({ isLoading: false });
		expect(result.current).toBe(false);
	});

	it("stays open when a refetch goes back to loading", () => {
		const { result, rerender } = renderHook(
			({ isLoading }) => useFirstLoad(isLoading),
			{ initialProps: { isLoading: false } },
		);

		rerender({ isLoading: true });
		expect(result.current).toBe(false);
	});
});
