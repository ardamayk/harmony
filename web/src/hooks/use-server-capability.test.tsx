import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import {
	useServerCapability,
	useServerCapabilityState,
} from "./use-server-capability";

const getHealth = vi.fn();

vi.mock("#/lib/api", () => ({
	apiClient: {
		getHealth: (...args: unknown[]) => getHealth(...args),
	},
}));

function createWrapper() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
	);
}

describe("useServerCapability", () => {
	beforeEach(() => {
		getHealth.mockReset();
	});

	it("reports advertised capabilities and ignores unknown extras", async () => {
		getHealth.mockResolvedValue({
			status: "ok",
			version: "test",
			capabilities: [
				"api.v1",
				"managed-import.v1",
				"future.unknown-feature.v7",
			],
		});
		const { result } = renderHook(
			() => ({
				managedImport: useServerCapability("managed-import.v1"),
				missing: useServerCapabilityState("future.missing-feature.v1"),
				unknown: useServerCapability("future.unknown-feature.v7"),
			}),
			{ wrapper: createWrapper() },
		);

		expect(result.current.missing).toBe("unknown");
		await waitFor(() => expect(result.current.managedImport).toBe(true));
		expect(result.current.missing).toBe("missing");
		expect(result.current.unknown).toBe(true);
		expect(getHealth).toHaveBeenCalledTimes(1);
	});

	it("stays unknown instead of missing while the health request fails", async () => {
		getHealth.mockRejectedValue(new Error("offline"));
		const { result } = renderHook(
			() => useServerCapabilityState("managed-import.v1"),
			{
				wrapper: createWrapper(),
			},
		);

		await waitFor(() => expect(getHealth).toHaveBeenCalled());
		expect(result.current).toBe("unknown");
	});
});
