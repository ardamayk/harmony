import {
	CLIENT_ERRORS_CAPABILITY,
	hasServerCapability,
} from "@repo/api-client";
import { useEffect } from "react";
import { isDesktopClient } from "#/desktop/bridge";
import { useHealth } from "#/hooks/use-server-capability";
import { apiClient } from "#/lib/api";

/**
 * The SDK sends nothing without a DSN. Envelopes never go to it: the Music
 * Server sends their errors with its own DSN (ADR 0043).
 */
const PLACEHOLDER_DSN = "https://public@music-server.invalid/0";
/** Clicks and console lines can name what is playing; requests are scrubbed. */
const KEPT_BREADCRUMBS = new Set(["fetch", "xhr", "navigation"]);

let started = false;
let capture: ((error: unknown) => void) | undefined;

/** Reports an error a boundary caught; a no-op until Sentry is started. */
export function reportClientError(error: unknown) {
	capture?.(error);
}

async function startClientErrors(version: string) {
	if (started) return;
	started = true;
	// Loaded only when the Music Server asks for errors, so the page stays light.
	const Sentry = await import("./sentry-sdk");
	Sentry.init({
		dsn: PLACEHOLDER_DSN,
		// shortcut: the Music Server's version, which is the Web Client's own when
		// it serves it; a Desktop Client's renderer may be another build.
		release: `earthly-web@${version}`,
		// Nothing about the person, their requests or the page's query; the
		// Music Server also drops the request and scrubs URLs and paths.
		dataCollection: {
			userInfo: false,
			cookies: false,
			httpHeaders: false,
			httpBodies: [],
			urlQueryParams: false,
			stackFrameVariables: false,
		},
		sendClientReports: false,
		integrations: (defaults) =>
			defaults.filter(
				(integration) =>
					integration.name !== "BrowserSession" &&
					integration.name !== "HttpContext",
			),
		beforeBreadcrumb: (breadcrumb) =>
			KEPT_BREADCRUMBS.has(breadcrumb.category ?? "") ? breadcrumb : null,
		initialScope: {
			tags: { client: isDesktopClient() ? "desktop" : "web" },
		},
		transport: (options) =>
			Sentry.createTransport(options, async ({ body }) => {
				await apiClient.reportClientErrors(
					typeof body === "string" ? body : body.slice(),
				);
				return {};
			}),
	});
	capture = (error) => Sentry.captureException(error);
}

/** Starts error reporting once the Owner's Music Server advertises it. */
export function useClientErrors() {
	const health = useHealth().data;
	const version =
		health && hasServerCapability(health, CLIENT_ERRORS_CAPABILITY)
			? health.version
			: undefined;
	useEffect(() => {
		if (version) void startClientErrors(version);
	}, [version]);
}
