// Serves the built web UI (web/dist) at app://earthly/. Paths without a
// matching file fall back to index.html so TanStack Router's history routes
// (/library/albums, /mini, ...) load on reload and in the mini window.

import { stat } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { CONTENT_SECURITY_POLICY, RENDERER_HOST } from "./security";

/**
 * Maps a request URL to a file under `root`, or `null` when the request is
 * not for the renderer host or tries to leave `root`.
 */
export function resolveRendererPath(root: string, url: string): string | null {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return null;
	}
	if (parsed.host !== RENDERER_HOST) return null;
	let pathname: string;
	try {
		pathname = decodeURIComponent(parsed.pathname);
	} catch {
		return null;
	}
	const resolved = normalize(join(root, pathname));
	const base = root.endsWith(sep) ? root : root + sep;
	if (resolved !== root && !resolved.startsWith(base)) return null;
	return resolved;
}

async function isFile(path: string): Promise<boolean> {
	try {
		return (await stat(path)).isFile();
	} catch {
		return false;
	}
}

/** The file to serve for `url`: the asset itself, or the SPA entry. */
export async function rendererFile(
	root: string,
	url: string,
): Promise<string | null> {
	const path = resolveRendererPath(root, url);
	if (path === null) return null;
	if (await isFile(path)) return path;
	// A missing asset is a 404; a missing route is the SPA.
	if (extname(path) !== "") return null;
	return join(root, "index.html");
}

export function createRendererHandler(
	root: string,
	fetchFile: (url: string) => Promise<Response>,
) {
	return async (request: Request): Promise<Response> => {
		if (request.method !== "GET" && request.method !== "HEAD") {
			return new Response(null, { status: 405 });
		}
		const file = await rendererFile(root, request.url);
		if (file === null) return new Response(null, { status: 404 });
		const response = await fetchFile(pathToFileURL(file).toString());
		const headers = new Headers(response.headers);
		headers.set("Content-Security-Policy", CONTENT_SECURITY_POLICY);
		headers.set("X-Content-Type-Options", "nosniff");
		return new Response(response.body, { status: response.status, headers });
	};
}
