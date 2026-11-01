import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { useCloseOnNavigate } from "./use-close-on-navigate";

type Listener = (event: { pathChanged: boolean }) => void;
let listener: Listener | undefined;
const unsubscribe = vi.fn();

vi.mock("@tanstack/react-router", () => ({
	useRouter: () => ({
		subscribe: (_event: string, next: Listener) => {
			listener = next;
			return unsubscribe;
		},
	}),
}));

describe("useCloseOnNavigate", () => {
	afterEach(cleanup);

	it("closes on a new path, not on a search change, and unsubscribes", () => {
		const onClose = vi.fn();
		const { unmount } = renderHook(() => useCloseOnNavigate(onClose));
		listener?.({ pathChanged: false });
		expect(onClose).not.toHaveBeenCalled();
		listener?.({ pathChanged: true });
		expect(onClose).toHaveBeenCalledOnce();
		unmount();
		expect(unsubscribe).toHaveBeenCalledOnce();
	});
});
