import type { components } from "./generated/schema";

type HealthResponse = components["schemas"]["HealthResponse"];

/**
 * Server Capabilities advertised by `GET /api/v1/health` (ADR 0006).
 *
 * Clients gate optional behavior on the exact capability name and must
 * ignore entries they do not recognize so a newer Music Server stays
 * compatible with an older Playback Client.
 */
export const API_V1_CAPABILITY = "api.v1";
export const MANAGED_IMPORT_CAPABILITY = "managed-import.v1";
export const MANAGED_IMPORT_BATCHES_CAPABILITY = "managed-import-batches.v1";
export const MANAGED_TRACK_DELETION_CAPABILITY = "managed-track-deletion.v1";
export const MANAGED_TRACK_REPLACEMENT_CAPABILITY =
	"managed-track-replacement.v1";
export const MANAGED_ALBUM_DELETION_CAPABILITY = "managed-album-deletion.v1";
/** Lazily generated waveform peaks; only advertised when ffmpeg is installed. */
export const TRACK_WAVEFORM_CAPABILITY = "track-waveform.v1";

export type ServerCapabilities =
	| HealthResponse
	| readonly string[]
	| null
	| undefined;

function capabilityList(source: ServerCapabilities): readonly string[] {
	if (!source) return [];
	return Array.isArray(source)
		? source
		: (source as HealthResponse).capabilities;
}

/**
 * Reports whether the Music Server advertises one exact capability name.
 * Unknown or additional capabilities never affect the result.
 */
export function hasServerCapability(
	source: ServerCapabilities,
	capability: string,
): boolean {
	return capabilityList(source).includes(capability);
}

/**
 * Lists the required capabilities a Music Server does not advertise so a
 * client can fail clearly instead of discovering the gap at request time.
 */
export function missingServerCapabilities(
	source: ServerCapabilities,
	required: readonly string[],
): string[] {
	const available = capabilityList(source);
	return required.filter((capability) => !available.includes(capability));
}
