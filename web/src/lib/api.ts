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
	transport: isDesktop ? desktopFetch : undefined,
	// Contract checks in development and tests only; production builds skip
	// them (library lists can hold thousands of items). See ADR 0034.
	validateResponses: import.meta.env.DEV,
});
