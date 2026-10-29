import {
	EXTERNAL_SOURCE_CAPABILITY,
	hasServerCapability,
} from "@repo/api-client";
import { useQuery } from "@tanstack/react-query";
import { apiClient } from "#/lib/api";

/**
 * "unknown" while the health response is still loading or failed, so callers
 * can avoid hiding controls before the Music Server has answered.
 */
export type ServerCapabilityState = "unknown" | "available" | "missing";

function useHealth() {
	return useQuery({
		queryKey: ["health"],
		queryFn: () => apiClient.getHealth(),
		staleTime: Number.POSITIVE_INFINITY,
	});
}

export function useServerCapabilityState(
	capability: string,
): ServerCapabilityState {
	const health = useHealth();
	if (!health.data) return "unknown";
	return hasServerCapability(health.data, capability) ? "available" : "missing";
}

export function useServerCapability(capability: string): boolean {
	return useServerCapabilityState(capability) === "available";
}

/**
 * The site the operator's External Source searches, such as SoundCloud
 * (ADR 0032); undefined unless the Music Server advertises the source and
 * names it.
 */
export function useExternalSourceName(): string | undefined {
	const health = useHealth();
	if (!hasServerCapability(health.data, EXTERNAL_SOURCE_CAPABILITY))
		return undefined;
	return health.data?.externalSource?.name || undefined;
}
