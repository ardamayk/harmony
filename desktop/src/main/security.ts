// The renderer trust boundary. The web UI runs as untrusted content: it gets
// no Node.js, a sandboxed preload with one narrow API, a CSP that only lets
// it reach its own bundle and the Rust core's private media proxy, and an
// allowlist of commands (see @repo/desktop-bridge). Music Server requests go
// through the core, which only talks to the saved exact origin.

import type { WebPreferences } from "electron";

/** The production renderer origin, served by the app:// protocol handler. */
export const RENDERER_SCHEME = "app";
export const RENDERER_HOST = "earthly";
export const RENDERER_ORIGIN = `${RENDERER_SCHEME}://${RENDERER_HOST}`;

/** Dedicated loopback origin of the core's tokenized media proxy. */
export const MEDIA_PROXY_ORIGIN = "http://127.0.0.1:43129";

export const CONTENT_SECURITY_POLICY = [
	"default-src 'self'",
	`connect-src 'self' ${MEDIA_PROXY_ORIGIN}`,
	`img-src 'self' ${MEDIA_PROXY_ORIGIN} data:`,
	`media-src ${MEDIA_PROXY_ORIGIN}`,
	"style-src 'self' 'unsafe-inline'",
	"font-src 'self' data:",
	"object-src 'none'",
	"base-uri 'none'",
	"frame-ancestors 'none'",
].join("; ");

export function rendererWebPreferences(preload: string): WebPreferences {
	return {
		preload,
		contextIsolation: true,
		sandbox: true,
		nodeIntegration: false,
		nodeIntegrationInWorker: false,
		webSecurity: true,
		allowRunningInsecureContent: false,
		spellcheck: false,
	};
}

/**
 * Whether a frame URL belongs to the app: the bundled renderer, or the Vite
 * dev server origin during development. IPC from anything else is refused.
 */
export function isTrustedRendererUrl(
	url: string,
	devServerOrigin: string | null,
): boolean {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return false;
	}
	if (parsed.protocol === `${RENDERER_SCHEME}:`) {
		return parsed.host === RENDERER_HOST;
	}
	return devServerOrigin !== null && parsed.origin === devServerOrigin;
}

/**
 * External links (Spotify, Last.fm, ...) leave the Desktop Client and open
 * in the system browser. Only web URLs are accepted, so renderer content
 * cannot hand arbitrary schemes to the operating system.
 */
export function validateExternalUrl(raw: unknown): string {
	if (typeof raw !== "string") {
		throw new Error("External link is not a valid URL.");
	}
	let parsed: URL;
	try {
		parsed = new URL(raw);
	} catch {
		throw new Error("External link is not a valid URL.");
	}
	if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
		throw new Error(
			`External link scheme '${parsed.protocol.replace(/:$/, "")}' is not allowed.`,
		);
	}
	return parsed.toString();
}
