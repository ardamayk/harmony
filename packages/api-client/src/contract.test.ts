import { describe, expect, expectTypeOf, it, vi } from "vite-plus/test";
import type { components, operations } from "./generated/schema";
import {
	createApiClient,
	hasServerCapability,
	MANAGED_IMPORT_CAPABILITY,
	missingServerCapabilities,
} from "./index";

type Schemas = components["schemas"];

/**
 * These tests pin the generated Managed Import contract so a regenerated
 * client that silently changes a documented shape fails type-checking here
 * instead of inside a Playback Client.
 */
describe("generated Managed Import contract", () => {
	it("types the Import Job lifecycle", () => {
		expectTypeOf<Schemas["ManagedImportJob"]["status"]>().toEqualTypeOf<
			"uploading" | "awaiting_confirmation" | "committed" | "failed"
		>();
		expectTypeOf<
			Schemas["ManagedImportJob"]["revision"]
		>().toEqualTypeOf<number>();
		expectTypeOf<Schemas["ManagedImportJob"]["trackId"]>().toEqualTypeOf<
			string | undefined
		>();
		expectTypeOf<Schemas["ManagedImportJobCreate"]>().toHaveProperty("batchId");
		expectTypeOf<Schemas["ManagedImportJobCreate"]>().toHaveProperty(
			"clientFileId",
		);
	});

	it("types per-file batch status and outcomes", () => {
		expectTypeOf<Schemas["ManagedImportBatchFile"]["state"]>().toEqualTypeOf<
			"accepted" | "rejected" | "unresolved" | "completed"
		>();
		expectTypeOf<Schemas["ManagedImportBatchFile"]["outcome"]>().toEqualTypeOf<
			| "imported"
			| "rejected"
			| "failed"
			| "replaced"
			| "not_attempted"
			| undefined
		>();
		expectTypeOf<Schemas["ManagedImportBatch"]["files"]>().toEqualTypeOf<
			Schemas["ManagedImportBatchFile"][]
		>();
	});

	it("types preview revisions and duplicate decisions", () => {
		expectTypeOf<
			Schemas["ManagedImportPreview"]["revision"]
		>().toEqualTypeOf<number>();
		expectTypeOf<
			Schemas["ManagedImportPreview"]["duplicateClassification"]
		>().toEqualTypeOf<"none" | "exact_duplicate">();
		expectTypeOf<
			Schemas["ManagedImportDuplicateDecision"]["action"]
		>().toEqualTypeOf<
			"import_separately" | "replace_existing" | "do_not_import"
		>();
		expectTypeOf<
			Schemas["ManagedImportBatchConfirmation"]["duplicateDecisions"]
		>().toEqualTypeOf<
			Schemas["ManagedImportDuplicateDecision"][] | undefined
		>();
		expectTypeOf<
			Schemas["ManagedImportConfirmation"]["revision"]
		>().toEqualTypeOf<number>();
	});

	it("types Import History, Track Replacement, and deletion", () => {
		expectTypeOf<Schemas["ManagedImportHistoryList"]["items"]>().toEqualTypeOf<
			Schemas["ManagedImportHistoryItem"][]
		>();
		expectTypeOf<
			Schemas["ManagedImportHistoryFile"]["resultCode"]
		>().toEqualTypeOf<string>();
		expectTypeOf<
			Schemas["TrackReplacementPreview"]["confirmationToken"]
		>().toEqualTypeOf<string>();
		expectTypeOf<
			Schemas["TrackReplacementPreview"]["metadata"]
		>().toEqualTypeOf<Schemas["TrackReplacementFieldDiff"][]>();
		expectTypeOf<Schemas["TrackReplacementConfirmation"]>().toHaveProperty(
			"confirmationToken",
		);
		expectTypeOf<
			Schemas["TrackDeletionPreview"]["confirmationToken"]
		>().toEqualTypeOf<string>();
		expectTypeOf<
			Schemas["TrackDeletionConfirmation"]["confirmationToken"]
		>().toEqualTypeOf<string>();
	});

	it("documents every supported binary upload media type", () => {
		type UploadBodies = NonNullable<
			operations["uploadManagedImportFile"]["requestBody"]
		>["content"];
		expectTypeOf<keyof UploadBodies>().toEqualTypeOf<
			| "application/octet-stream"
			| "audio/flac"
			| "audio/wav"
			| "audio/mpeg"
			| "audio/mp4"
			| "audio/ogg"
			| "audio/opus"
		>();
		expectTypeOf<
			operations["uploadManagedImportFile"]["parameters"]["header"]["X-Import-Filename"]
		>().toEqualTypeOf<string>();
		expectTypeOf<
			operations["uploadManagedImportFile"]["parameters"]["header"]["X-Import-Filename-Encoding"]
		>().toEqualTypeOf<"url" | undefined>();
	});

	it("keeps structured errors separate from human-readable messages", () => {
		expectTypeOf<Schemas["ErrorResponse"]>().toEqualTypeOf<{
			error: string;
			code: string;
			message: string;
			field?: string;
			reason?: string;
			issues?: { code: string; field: string; reason: string }[];
		}>();
		expectTypeOf<
			operations["uploadManagedImportFile"]["responses"][422]["content"]["application/json"]
		>().toEqualTypeOf<Schemas["ErrorResponse"]>();
	});

	it("exposes one client method per Managed Import operation", () => {
		const client = createApiClient({ baseUrl: "http://music.test" });
		const wrappers: Record<
			keyof Pick<
				operations,
				| "createManagedImportBatch"
				| "getManagedImportBatch"
				| "cancelManagedImportBatch"
				| "confirmManagedImportBatch"
				| "createManagedImportJob"
				| "getManagedImportJob"
				| "cancelManagedImport"
				| "uploadManagedImportFile"
				| "confirmManagedImport"
				| "listImportHistory"
				| "previewTrackDeletion"
				| "deleteTrack"
				| "previewAlbumDeletion"
				| "deleteAlbum"
				| "createTrackReplacement"
				| "confirmTrackReplacement"
			>,
			keyof typeof client
		> = {
			createManagedImportBatch: "createManagedImportBatch",
			getManagedImportBatch: "getManagedImportBatch",
			cancelManagedImportBatch: "cancelManagedImportBatch",
			confirmManagedImportBatch: "confirmManagedImportBatch",
			createManagedImportJob: "createManagedImportJob",
			getManagedImportJob: "getManagedImportJob",
			cancelManagedImport: "cancelManagedImport",
			uploadManagedImportFile: "uploadManagedImportFile",
			confirmManagedImport: "confirmManagedImport",
			listImportHistory: "listImportHistory",
			previewTrackDeletion: "previewTrackDeletion",
			deleteTrack: "deleteTrack",
			previewAlbumDeletion: "previewAlbumDeletion",
			deleteAlbum: "deleteAlbum",
			createTrackReplacement: "createTrackReplacement",
			confirmTrackReplacement: "confirmTrackReplacement",
		};
		for (const method of Object.values(wrappers)) {
			expect(client[method]).toBeTypeOf("function");
		}
	});
});

describe("capability negotiation at the client seam", () => {
	it("reads capabilities from health and ignores unknown entries and fields", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			new Response(
				JSON.stringify({
					status: "ok",
					version: "9.9.9",
					capabilities: [
						"api.v1",
						MANAGED_IMPORT_CAPABILITY,
						"future.optional.v3",
					],
					experimental: { ignored: true },
				}),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			),
		);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		const health = await client.getHealth();

		expect(hasServerCapability(health, MANAGED_IMPORT_CAPABILITY)).toBe(true);
		expect(hasServerCapability(health, "library-migration.v1")).toBe(false);
		expect(missingServerCapabilities(health, ["library-migration.v1"])).toEqual(
			["library-migration.v1"],
		);
		expect(hasServerCapability(health, "future.optional.v3")).toBe(true);
	});
});
