import type { QueueEvent } from "@repo/api-client";
import { DESKTOP_EVENTS, type ImportProgressEvent } from "@repo/desktop-bridge";
import { hasDesktopBridge, invoke, listen, type Unlisten } from "./desktop-api";

let mediaProxyBaseUrl: string | null = null;

export type ServerConnection = {
	origin: string;
};

export type ConnectionCheck = ServerConnection & {
	version: string;
	capabilities: string[];
};

type BridgeHttpResponse = {
	status: number;
	headers: Record<string, string>;
	body: number[];
};

export type DesktopImportSelection = {
	id: string;
	name: string;
	size: number;
};

export function isDesktopClient(): boolean {
	return hasDesktopBridge();
}

export type DesktopMpvStatus = {
	/** The mpv executable starts and answers --version. */
	available: boolean;
	/** The found version equals the pinned version the Desktop Client requires. */
	pinned: boolean;
	pinnedVersion: string;
	version?: string | null;
	detail?: string | null;
};

/** Pinned mpv sidecar status for the Settings page; Desktop Client only. */
export function getDesktopMpvStatus(): Promise<DesktopMpvStatus> {
	return invoke("get_desktop_mpv_status");
}

/** Open a web URL in the system browser; Desktop Client only. */
export function openExternalUrl(url: string): Promise<void> {
	return invoke("desktop_open_external_url", { url });
}

export function getServerConnection(): Promise<ServerConnection | null> {
	return invoke("get_server_connection");
}

export function testServerConnection(origin: string): Promise<ConnectionCheck> {
	return invoke("test_server_connection", { origin });
}

export function saveServerConnection(origin: string): Promise<ConnectionCheck> {
	return invoke("save_server_connection", { origin });
}

export function selectDesktopImportFiles(): Promise<DesktopImportSelection[]> {
	return invoke("desktop_select_import_files");
}

export function selectDesktopImportFolder(): Promise<DesktopImportSelection[]> {
	return invoke("desktop_select_import_folder");
}

export function cancelDesktopImportUpload(uploadId: string): Promise<void> {
	return invoke("desktop_cancel_import_upload", { uploadId });
}

export function releaseDesktopImportSelections(
	selectionIds: string[],
): Promise<void> {
	return invoke("desktop_release_import_selections", { selectionIds });
}

export async function desktopUploadImportFile(
	selectionId: string,
	jobId: string,
	onProgress?: (progress: number, transferredBytes?: number) => void,
	signal?: AbortSignal,
): Promise<Response> {
	if (signal?.aborted) {
		throw new DOMException("Managed Import upload canceled", "AbortError");
	}
	const uploadId = crypto.randomUUID();
	const stopProgress = listen<ImportProgressEvent>(
		DESKTOP_EVENTS.importProgress,
		(progress) => {
			if (progress.uploadId !== uploadId || progress.totalBytes <= 0) return;
			onProgress?.(
				Math.round((progress.sentBytes / progress.totalBytes) * 100),
				progress.sentBytes,
			);
		},
	);
	const handleAbort = () => {
		cancelDesktopImportUpload(uploadId).catch((error) => {
			console.error("Desktop Managed Import cancellation failed", error);
		});
	};
	signal?.addEventListener("abort", handleAbort, { once: true });
	onProgress?.(0);
	try {
		const response = await invoke<BridgeHttpResponse>(
			"desktop_upload_import_file",
			{
				selectionId,
				uploadId,
				jobId,
			},
		);
		if (signal?.aborted) {
			throw new DOMException("Managed Import upload canceled", "AbortError");
		}
		onProgress?.(100);
		return bridgeResponse(response);
	} finally {
		stopProgress();
		signal?.removeEventListener("abort", handleAbort);
	}
}

export async function initializeMediaProxy(): Promise<string> {
	const value = await invoke<string>("get_media_proxy_url");
	const url = new URL(value);
	const isValid =
		url.protocol === "http:" &&
		url.hostname === "127.0.0.1" &&
		url.port.length > 0 &&
		/^\/[a-f0-9]{64}$/.test(url.pathname) &&
		url.search === "" &&
		url.hash === "";
	if (!isValid) {
		throw new TypeError("Desktop media proxy returned an invalid loopback URL");
	}
	mediaProxyBaseUrl = url.toString().replace(/\/$/, "");
	return mediaProxyBaseUrl;
}

export function getMediaProxyBaseUrl(): string {
	if (!mediaProxyBaseUrl) {
		throw new Error("Desktop media proxy is not initialized");
	}
	return mediaProxyBaseUrl;
}

export async function listenForServerConnectionChanges(
	callback: (connection: ConnectionCheck) => void,
): Promise<Unlisten> {
	return listen<ConnectionCheck>(DESKTOP_EVENTS.connectionChanged, callback);
}

export async function listenForQueueEvents(
	onEvent: (event: QueueEvent) => void,
	onError: (error: Error) => void,
): Promise<Unlisten> {
	const unlistenQueue = listen<QueueEvent>(
		DESKTOP_EVENTS.queueInvalidated,
		onEvent,
	);
	let unlistenError: Unlisten | undefined;
	try {
		unlistenError = listen<string>(
			DESKTOP_EVENTS.queueEventsError,
			(message) => {
				onError(new Error(`Desktop Queue event stream: ${message}`));
			},
		);
		await invoke("desktop_reconnect_queue_events");
		return () => {
			unlistenQueue();
			unlistenError?.();
		};
	} catch (error) {
		unlistenQueue();
		unlistenError?.();
		throw error;
	}
}

export async function desktopFetch(
	input: RequestInfo | URL,
	init?: RequestInit,
): Promise<Response> {
	const url = input instanceof Request ? input.url : input.toString();
	const headers = new Headers(
		init?.headers ?? (input instanceof Request ? input.headers : undefined),
	);
	const normalizedHeaders = Object.fromEntries(
		Array.from(headers.entries(), ([name, value]) => [
			name.toLowerCase(),
			value,
		]),
	);
	const body = await encodeBody(init?.body);
	const response = await invoke<BridgeHttpResponse>("desktop_http_request", {
		request: {
			method: init?.method ?? (input instanceof Request ? input.method : "GET"),
			url,
			headers: normalizedHeaders,
			body,
		},
	});

	return bridgeResponse(response);
}

function bridgeResponse(response: BridgeHttpResponse): Response {
	const isBodyless = [204, 205, 304].includes(response.status);
	return new Response(isBodyless ? null : new Uint8Array(response.body), {
		status: response.status,
		headers: response.headers,
	});
}

async function encodeBody(
	body: BodyInit | null | undefined,
): Promise<number[] | null> {
	if (body == null) return null;
	if (typeof body === "string")
		return Array.from(new TextEncoder().encode(body));
	if (body instanceof URLSearchParams) {
		return Array.from(new TextEncoder().encode(body.toString()));
	}
	if (body instanceof Blob)
		return Array.from(new Uint8Array(await body.arrayBuffer()));
	if (body instanceof ArrayBuffer) return Array.from(new Uint8Array(body));
	if (ArrayBuffer.isView(body)) {
		return Array.from(
			new Uint8Array(body.buffer, body.byteOffset, body.byteLength),
		);
	}
	throw new TypeError(
		"Desktop HTTP transport does not support streaming or FormData bodies",
	);
}
