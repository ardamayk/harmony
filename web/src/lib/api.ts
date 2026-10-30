import { createApiClient } from "@repo/api-client";
import {
	desktopFetch,
	getMediaProxyBaseUrl,
	isDesktopClient,
	listenForQueueEvents,
} from "#/desktop/bridge";
import { getApiBaseUrl } from "#/env";

const isDesktop = isDesktopClient();
export const apiClient = createApiClient({
	baseUrl: isDesktop ? "" : getApiBaseUrl(),
	mediaBaseUrl: isDesktop ? getMediaProxyBaseUrl : undefined,
	queueEventSubscriber: isDesktop ? listenForQueueEvents : undefined,
	// The Desktop renderer's CSP allows no connection to the Music Server, and
	// its requests pass through the core one whole response at a time.
	roomEventsMode: isDesktop ? "poll" : undefined,
	transport: isDesktop ? desktopFetch : undefined,
	// Contract checks in development and tests only; production builds skip
	// them (library lists can hold thousands of items). See ADR 0034.
	validateResponses: import.meta.env.DEV,
});
