import { hasServerCapability } from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { apiClient } from "#/lib/api";

/**
 * "unknown" while the health response is still loading or failed, so callers
 * can avoid hiding controls before the Music Server has answered.
 */
export type ServerCapabilityState = "unknown" | "available" | "missing";

export function useServerCapabilityState(
	capability: string,
): ServerCapabilityState {
	const health = useQuery({
		queryKey: ["health"],
		queryFn: () => apiClient.getHealth(),
		staleTime: Number.POSITIVE_INFINITY,
	});
	if (!health.data) return "unknown";
	return hasServerCapability(health.data, capability) ? "available" : "missing";
}

export function useServerCapability(capability: string): boolean {
	return useServerCapabilityState(capability) === "available";
}
