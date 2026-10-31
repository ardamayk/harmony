import { ApiError, type ManagedImportPreview } from "@repo/api-client";

/**
 * Reads the Music Server's answer to a Desktop upload. A proxy in front of it
 * answers a refusal with an HTML page, so only a success has to be JSON.
 */
export async function readDesktopUpload(
	response: Response,
): Promise<ManagedImportPreview> {
	if (!response.ok) {
		const body = await response.json().catch(() => ({
			error: "unknown",
			code: "unknown",
			message: `HTTP ${response.status}`,
		}));
		throw new ApiError(response.status, body);
	}
	return response.json();
}
