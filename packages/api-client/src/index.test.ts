import { describe, expect, expectTypeOf, it, vi } from "vite-plus/test";
import type { components, operations } from "./generated/schema";
import {
	type Album,
	type ArtistCredit,
	createApiClient,
	type Genre,
	type ReleaseIdentifier,
	type Track,
} from "./index";

describe("normalized library contracts", () => {
	it("exposes ordered Artist credits, structured Genres, disc positions, editions, and artwork", () => {
		expectTypeOf<Album>().toHaveProperty("albumArtists");
		expectTypeOf<Album>().toHaveProperty("genreItems");
		expectTypeOf<Album>().toHaveProperty("releaseDate");
		expectTypeOf<Album>().toHaveProperty("releaseIdentifiers");
		expectTypeOf<Album>().toHaveProperty("artwork");
		expectTypeOf<Track>().toHaveProperty("artists");
		expectTypeOf<Track>().toHaveProperty("genres");
		expectTypeOf<Track>().toHaveProperty("discNo");
		expectTypeOf<Album["albumArtists"]>().toEqualTypeOf<ArtistCredit[]>();
		expectTypeOf<Album["genreItems"]>().toEqualTypeOf<Genre[]>();
		expectTypeOf<Album["releaseIdentifiers"]>().toEqualTypeOf<
			ReleaseIdentifier[]
		>();
		expectTypeOf<Track["artists"]>().toEqualTypeOf<ArtistCredit[]>();
		expectTypeOf<Track["genres"]>().toEqualTypeOf<Genre[]>();
		expectTypeOf<Track["discNo"]>().toEqualTypeOf<number>();
	});
});

describe("Track ReplayGain Metadata", () => {
	it("preserves missing values as null", () => {
		const replayGain: Track["replayGain"] = {
			trackGainDb: -7.25,
			trackPeak: null,
			albumGainDb: null,
			albumPeak: 1.01,
		};

		expect(replayGain).toEqual({
			trackGainDb: -7.25,
			trackPeak: null,
			albumGainDb: null,
			albumPeak: 1.01,
		});
	});
});

describe("Managed Import media contracts", () => {
	it("requires source bit depth for FLAC", () => {
		expectTypeOf<
			components["schemas"]["ManagedImportFlacPreviewFile"]["bitDepth"]
		>().toEqualTypeOf<number>();
	});

	it("supports MP3 without a source bit depth", () => {
		const file: components["schemas"]["ManagedImportPreviewFile"] = {
			originalFilename: "fixture.mp3",
			title: "MP3 Inspection Fixture",
			artists: ["Primary Artist"],
			albumArtists: ["Album Artist"],
			album: "Strict MP3 Import Tests",
			genres: ["Electronic"],
			trackNo: 2,
			discNo: 1,
			durationMs: 626,
			format: "mp3",
			container: "mp3",
			codec: "mp3",
			sampleRateHz: 22_050,
			channelCount: 1,
			bitrateKbps: 52,
			artworkMediaType: "image/png",
		};

		expect("bitDepth" in file).toBe(false);
	});
});

describe("createApiClient", () => {
	it("keeps Library Search total optional and expands only on request", async () => {
		type SearchResponse = components["schemas"]["LibrarySearchResponse"];
		expectTypeOf<SearchResponse["total"]>().toEqualTypeOf<number | undefined>();
		expectTypeOf<
			operations["searchLibrary"]["parameters"]["query"]["all"]
		>().toEqualTypeOf<boolean | undefined>();
		const legacy: SearchResponse = {
			tracks: [],
			albums: [],
			artists: [],
			genres: [],
			playlists: [],
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(Response.json(legacy))
			.mockResolvedValueOnce(Response.json({ ...legacy, total: 0 }))
			.mockResolvedValueOnce(Response.json({ ...legacy, total: 0 }));
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await expect(client.searchLibrary("AC/DC & blue")).resolves.toEqual(legacy);
		await expect(client.searchLibrary("AC/DC & blue", false)).resolves.toEqual({
			...legacy,
			total: 0,
		});
		await expect(client.searchLibrary("AC/DC & blue", true)).resolves.toEqual({
			...legacy,
			total: 0,
		});
		expect(transport.mock.calls.map(([input]) => input)).toEqual([
			"http://music.test/api/v1/library/search?q=AC%2FDC+%26+blue",
			"http://music.test/api/v1/library/search?q=AC%2FDC+%26+blue",
			"http://music.test/api/v1/library/search?q=AC%2FDC+%26+blue&all=true",
		]);
	});

	it("revalidates previously fresh waveform responses after Track Replacement", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			new Response(
				JSON.stringify({
					trackId: "00000000-0000-4000-8000-000000000001",
					peaks: [128],
					peakCount: 1,
				}),
			),
		);
		const client = createApiClient({ baseUrl: "http://music.test", transport });
		await client.getTrackWaveform("00000000-0000-4000-8000-000000000001");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/library/tracks/00000000-0000-4000-8000-000000000001/waveform",
			expect.objectContaining({ cache: "no-cache" }),
		);
	});

	it("reads a missing Artist biography as null and other failures as errors", async () => {
		const notFound = () =>
			Response.json(
				{ code: "not_found", message: "artist biography not found" },
				{ status: 404 },
			);
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(
				Response.json({ text: "A band.", sourceUrl: "https://w.example/a" }),
			)
			.mockResolvedValueOnce(notFound())
			.mockResolvedValueOnce(
				Response.json({ code: "internal", message: "boom" }, { status: 500 }),
			);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await expect(
			client.getArtistBiography("00000000-0000-4000-8000-000000000005"),
		).resolves.toEqual({
			text: "A band.",
			sourceUrl: "https://w.example/a",
		});
		await expect(
			client.getArtistBiography("00000000-0000-4000-8000-000000000005"),
		).resolves.toBeNull();
		await expect(
			client.getArtistBiography("00000000-0000-4000-8000-000000000005"),
		).rejects.toThrow("boom");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/library/artists/00000000-0000-4000-8000-000000000005/biography",
			expect.anything(),
		);
	});

	it("probes a Track stream with HEAD and reports the status without throwing", async () => {
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(new Response(null, { status: 404 }));
		const client = createApiClient({
			baseUrl: "http://music.test",
			streamBaseUrl: "http://127.0.0.1:43129/token",
			transport,
			getToken: () => "secret",
		});

		await expect(
			client.headTrackStream("00000000-0000-4000-8000-000000000001"),
		).resolves.toEqual({
			status: 404,
		});
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/tracks/00000000-0000-4000-8000-000000000001/stream",
			expect.objectContaining({ method: "HEAD" }),
		);
		const headers = new Headers(transport.mock.calls[0]?.[1]?.headers);
		expect(headers.get("Authorization")).toBe("Bearer secret");
	});

	it("previews and explicitly confirms Permanent Track Deletion", async () => {
		const preview = {
			trackId: "00000000-0000-4000-8000-000000000001",
			trackTitle: "Delete Me",
			managedFile: { path: "library/artist/album/track.flac", sizeBytes: 11 },
			playlistReferences: [],
			queueReferences: [],
			confirmationToken: "token-1",
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(
				new Response(JSON.stringify(preview), { status: 200 }),
			)
			.mockResolvedValueOnce(
				new Response(JSON.stringify({ deletedFiles: 1 }), { status: 200 }),
			);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await expect(
			client.previewTrackDeletion("00000000-0000-4000-8000-000000000001"),
		).resolves.toEqual(preview);
		await expect(
			client.deleteTrack("00000000-0000-4000-8000-000000000001", "token-1"),
		).resolves.toEqual({
			deletedFiles: 1,
		});

		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/library/tracks/00000000-0000-4000-8000-000000000001",
			expect.objectContaining({
				method: "DELETE",
				body: JSON.stringify({ confirmationToken: "token-1" }),
			}),
		);
		const headers = new Headers(transport.mock.calls[1]?.[1]?.headers);
		expect(headers.get("X-Permanent-Delete")).toBe("1");
	});

	it("previews and explicitly confirms an Album deletion as per-Track deletions", async () => {
		const preview = {
			albumId: "00000000-0000-4000-8000-000000000004",
			albumTitle: "1989",
			trackCount: 2,
			totalSizeBytes: 22,
			tracks: [
				{
					trackId: "00000000-0000-4000-8000-000000000001",
					trackTitle: "First",
					sizeBytes: 11,
				},
				{
					trackId: "00000000-0000-4000-8000-000000000002",
					trackTitle: "Second",
					sizeBytes: 11,
				},
			],
			playlistReferences: [],
			queueReferences: [
				{ userId: "00000000-0000-4000-8000-000000000009", itemCount: 2 },
			],
			confirmationToken: "album-token",
		};
		const result = {
			deleted: preview.tracks,
			stoppedAt: null,
			deletedFiles: 2,
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(
				new Response(JSON.stringify(preview), { status: 200 }),
			)
			.mockResolvedValueOnce(
				new Response(JSON.stringify(result), { status: 200 }),
			);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await expect(
			client.previewAlbumDeletion("00000000-0000-4000-8000-000000000004"),
		).resolves.toEqual(preview);
		await expect(
			client.deleteAlbum("00000000-0000-4000-8000-000000000004", "album-token"),
		).resolves.toEqual(result);

		expect(transport).toHaveBeenNthCalledWith(
			1,
			"http://music.test/api/v1/library/albums/00000000-0000-4000-8000-000000000004/deletion",
			expect.anything(),
		);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/library/albums/00000000-0000-4000-8000-000000000004",
			expect.objectContaining({
				method: "DELETE",
				body: JSON.stringify({ confirmationToken: "album-token" }),
			}),
		);
		const headers = new Headers(transport.mock.calls[1]?.[1]?.headers);
		expect(headers.get("X-Permanent-Delete")).toBe("1");
	});

	it("creates and confirms a multi-file Managed Import Batch", async () => {
		const createdBatch = {
			id: "00000000-0000-4000-8000-000000000007",
			status: "uploading",
			revision: 1,
			files: [],
		};
		const completedBatch = {
			id: "00000000-0000-4000-8000-000000000007",
			status: "completed",
			revision: 4,
			files: [],
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(Response.json(createdBatch, { status: 201 }))
			.mockResolvedValueOnce(
				Response.json(
					{
						id: "00000000-0000-4000-8000-000000000003",
						status: "uploading",
						revision: 1,
						validationProgress: 0,
					},
					{ status: 201 },
				),
			)
			.mockResolvedValueOnce(Response.json(completedBatch));
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		const batch = await client.createManagedImportBatch();
		await client.createManagedImportJob(
			batch.id,
			"00000000-0000-4000-8000-000000000001",
		);
		const report = await client.confirmManagedImportBatch(
			batch.id,
			2,
			["00000000-0000-4000-8000-000000000003"],
			[
				{
					jobId: "00000000-0000-4000-8000-000000000003",
					action: "import_separately",
				},
			],
		);

		expect(report.status).toBe("completed");
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			batchId: "00000000-0000-4000-8000-000000000007",
			clientFileId: "00000000-0000-4000-8000-000000000001",
		});
		expect(JSON.parse(String(transport.mock.calls[2]?.[1]?.body))).toEqual({
			revision: 2,
			selectedFileIds: ["00000000-0000-4000-8000-000000000003"],
			duplicateDecisions: [
				{
					jobId: "00000000-0000-4000-8000-000000000003",
					action: "import_separately",
				},
			],
		});
	});

	it("polls observable Managed Import validation results", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			Response.json({
				id: "00000000-0000-4000-8000-000000000003",
				status: "failed",
				revision: 1,
				validationProgress: 40,
				errorCode: "validation_cancelled",
			}),
		);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		const result = await client.getManagedImportJob(
			"00000000-0000-4000-8000-000000000003",
		);

		expect(result.validationProgress).toBe(40);
		expect(result.errorCode).toBe("validation_cancelled");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/imports/00000000-0000-4000-8000-000000000003",
			expect.objectContaining({ headers: expect.any(Headers) }),
		);
	});

	it("cancels Managed Import staging through explicit DELETE requests", async () => {
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValue(new Response(null, { status: 204 }));
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await client.cancelManagedImportBatch(
			"00000000-0000-4000-8000-000000000007",
		);
		await client.cancelManagedImport("00000000-0000-4000-8000-000000000003");

		expect(transport).toHaveBeenNthCalledWith(
			1,
			"http://music.test/api/v1/import-batches/00000000-0000-4000-8000-000000000007",
			expect.objectContaining({ method: "DELETE" }),
		);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/imports/00000000-0000-4000-8000-000000000003",
			expect.objectContaining({ method: "DELETE" }),
		);
	});

	it("lists retained terminal Import History results", async () => {
		const payload = {
			items: [
				{
					importId: "00000000-0000-4000-8000-000000000007",
					startedAt: "2026-09-02T10:00:00Z",
					completedAt: "2026-09-02T10:01:00Z",
					resultCode: "partially_completed",
					counts: {
						total: 2,
						imported: 1,
						rejected: 1,
						failed: 0,
						replaced: 0,
						notAttempted: 0,
						canceled: 0,
					},
					files: [
						{
							fileId: "00000000-0000-4000-8000-000000000014",
							jobId: "00000000-0000-4000-8000-000000000013",
							safeFilename: "track.flac",
							startedAt: "2026-09-02T10:00:00Z",
							completedAt: "2026-09-02T10:01:00Z",
							contentSha256: "0".repeat(64),
							resultCode: "imported",
							createdTrackId: "00000000-0000-4000-8000-000000000001",
						},
					],
				},
			],
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValue(Response.json(payload));
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		const history = await client.listImportHistory();

		expect(history).toEqual(payload);
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/import-history",
			expect.objectContaining({ headers: expect.any(Headers) }),
		);
	});

	it("streams one Managed Import file and confirms its preview revision", async () => {
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(
				Response.json(
					{
						id: "00000000-0000-4000-8000-000000000003",
						status: "uploading",
						revision: 1,
						validationProgress: 0,
					},
					{ status: 201 },
				),
			)
			.mockResolvedValueOnce(
				Response.json({
					jobId: "00000000-0000-4000-8000-000000000003",
					status: "awaiting_confirmation",
					revision: 2,
					duplicateClassification: "none",
					file: {
						originalFilename: "fixture.flac",
						title: "Inspection Fixture",
						artists: ["Test Artist"],
						albumArtists: ["Test Album Artist"],
						album: "Strict Import Tests",
						genres: ["Electronic"],
						trackNo: 3,
						discNo: 1,
						durationMs: 250,
						format: "flac",
						container: "flac",
						codec: "flac",
						sampleRateHz: 44_100,
						channelCount: 1,
						bitDepth: 16,
						bitrateKbps: 134,
						artworkMediaType: "image/png",
					},
				}),
			)
			.mockResolvedValueOnce(
				Response.json({
					jobId: "00000000-0000-4000-8000-000000000003",
					status: "committed",
					revision: 3,
					trackId: "00000000-0000-4000-8000-000000000001",
				}),
			);
		const client = createApiClient({
			baseUrl: "http://music.test",
			transport,
		});
		const file = new Blob(["flac bytes"], { type: "audio/flac" });
		const progress: number[] = [];

		const job = await client.createManagedImportJob();
		const preview = await client.uploadManagedImportFile(
			job.id,
			"fixture.flac",
			file,
			(value) => progress.push(value),
		);
		const result = await client.confirmManagedImport(job.id, preview.revision);

		expect(result.trackId).toBe("00000000-0000-4000-8000-000000000001");
		expect(progress).toEqual([0, 100]);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/imports/00000000-0000-4000-8000-000000000003/file",
			expect.objectContaining({ method: "PUT", body: file }),
		);
		const uploadHeaders = new Headers(transport.mock.calls[1]?.[1]?.headers);
		expect(uploadHeaders.get("Content-Type")).toBe("audio/flac");
		expect(uploadHeaders.get("X-Import-Filename")).toBe("fixture.flac");
		expect(JSON.parse(String(transport.mock.calls[2]?.[1]?.body))).toEqual({
			revision: 2,
		});
	});

	it("uploads WAV Managed Imports with the audio/wav content type", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValueOnce(
			Response.json({
				jobId: "00000000-0000-4000-8000-000000000003",
				status: "awaiting_confirmation",
				revision: 2,
			}),
		);
		const client = createApiClient({
			baseUrl: "http://music.test",
			transport,
			// Only the request headers matter here, not the preview.
			validateResponses: false,
		});

		await client.uploadManagedImportFile(
			"00000000-0000-4000-8000-000000000003",
			"recording.wav",
			new Blob(["wav bytes"], { type: "audio/wav" }),
		);

		const uploadHeaders = new Headers(transport.mock.calls[0]?.[1]?.headers);
		expect(uploadHeaders.get("Content-Type")).toBe("audio/wav");
		expect(uploadHeaders.get("X-Import-Filename")).toBe("recording.wav");
	});

	it("preserves structured Managed Import validation fields", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			Response.json(
				{
					error: "invalid_metadata",
					code: "invalid_metadata",
					message: "File failed the Strict Import Profile at TITLE",
					field: "TITLE",
					reason: "required tag is missing",
				},
				{ status: 422 },
			),
		);
		const client = createApiClient({
			baseUrl: "http://music.test",
			transport,
		});

		await expect(
			client.uploadManagedImportFile(
				"00000000-0000-4000-8000-000000000003",
				"missing-title.flac",
				new Blob(["flac bytes"], { type: "audio/flac" }),
			),
		).rejects.toMatchObject({
			status: 422,
			body: {
				code: "invalid_metadata",
				field: "TITLE",
				reason: "required tag is missing",
			},
		});
	});

	it("normalizes legacy library responses at the client boundary", async () => {
		const transport = vi.fn<typeof fetch>(async (input) => {
			if (input.toString().includes("/library/albums/")) {
				return Response.json({
					id: "00000000-0000-4000-8000-000000000004",
					title: "Deprecated-shape Album",
					artistId: "00000000-0000-4000-8000-000000000005",
					artistName: "Deprecated-shape Artist",
					genres: ["Rock"],
					tracks: [
						{
							id: "00000000-0000-4000-8000-000000000001",
							title: "Deprecated-shape Track",
							artistName: "Track Artist",
							albumId: "00000000-0000-4000-8000-000000000004",
							durationMs: 1000,
							format: "flac",
							genre: "Electronic / Ambient",
						},
					],
				});
			}
			return Response.json({
				items: [
					{
						id: "00000000-0000-4000-8000-000000000001",
						title: "Deprecated-shape Track",
						artistName: "Track Artist",
						albumId: "00000000-0000-4000-8000-000000000004",
						durationMs: 1000,
						format: "flac",
						genre: "Electronic / Ambient",
					},
				],
				total: 1,
			});
		});
		const client = createApiClient({ baseUrl: "", transport });

		const album = await client.getAlbum("00000000-0000-4000-8000-000000000004");
		const tracks = await client.listTracks();

		expect(album.albumArtists).toEqual([
			{
				id: "00000000-0000-4000-8000-000000000005",
				name: "Deprecated-shape Artist",
			},
		]);
		expect(album.genreItems).toEqual([
			{ id: "legacy-genre:Rock", name: "Rock" },
		]);
		expect(album.releaseIdentifiers).toEqual([]);
		expect(album.tracks[0]).toMatchObject({
			artists: [{ id: "legacy-artist:Track Artist", name: "Track Artist" }],
			albumArtists: [],
			discNo: 1,
			genres: [
				{
					id: "legacy-genre:Electronic / Ambient",
					name: "Electronic / Ambient",
				},
			],
		});
		expect(tracks.items[0]?.discNo).toBe(1);
	});

	it("generates the missing revision response for Queue removal", () => {
		type RemoveQueueResponses =
			operations["removePlaybackQueueItem"]["responses"];

		expectTypeOf<RemoveQueueResponses>().toHaveProperty(400);
	});

	it("generates conflict responses for every Queue mutation", () => {
		type ReplaceResponses = operations["replacePlaybackQueue"]["responses"];
		type AppendResponses = operations["appendPlaybackQueueItem"]["responses"];
		type ReorderResponses = operations["reorderPlaybackQueue"]["responses"];
		type RemoveResponses = operations["removePlaybackQueueItem"]["responses"];

		expectTypeOf<ReplaceResponses>().toHaveProperty(409);
		expectTypeOf<AppendResponses>().toHaveProperty(409);
		expectTypeOf<ReorderResponses>().toHaveProperty(409);
		expectTypeOf<RemoveResponses>().toHaveProperty(409);
	});

	it("generates Queue event stream response", () => {
		type EventResponses = operations["streamPlaybackQueueEvents"]["responses"];

		expectTypeOf<EventResponses>().toHaveProperty(200);
	});

	it("subscribes to Queue invalidations and closes the event stream", async () => {
		const listeners = new Map<string, (event: MessageEvent<string>) => void>();
		const eventSource = {
			addEventListener: vi.fn(
				(name: string, listener: (event: MessageEvent<string>) => void) => {
					listeners.set(name, listener);
				},
			),
			close: vi.fn(),
			onerror: null,
		};
		const eventSourceFactory = vi.fn(() => eventSource);
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValue(
				serverHealthResponse(["api.v1", "playback.queue-events.v1"]),
			);
		const client = createApiClient({
			baseUrl: "",
			queueEventsBaseUrl: async () => "http://music.test",
			eventSourceFactory,
			transport,
		});
		const onEvent = vi.fn();

		const unsubscribe = client.subscribePlaybackQueueEvents(onEvent);
		await vi.waitFor(() => expect(eventSourceFactory).toHaveBeenCalled());
		listeners.get("queue-invalidated")?.({
			data: JSON.stringify({
				revision: "opaque-7",
				sequence: "7",
				invalidates: ["queue"],
			}),
		} as MessageEvent<string>);

		expect(eventSourceFactory).toHaveBeenCalledWith(
			"http://music.test/api/v1/playback/queue/events",
		);
		expect(onEvent).toHaveBeenCalledWith({
			revision: "opaque-7",
			sequence: "7",
			invalidates: ["queue"],
		});
		unsubscribe();
		expect(eventSource.close).toHaveBeenCalledOnce();
	});

	it("recovers from a transient capability check failure without remounting", async () => {
		vi.useFakeTimers();
		try {
			const transport = vi
				.fn<typeof fetch>()
				.mockRejectedValueOnce(new Error("temporary health failure"))
				.mockRejectedValueOnce(new Error("temporary health failure"))
				.mockResolvedValueOnce(
					serverHealthResponse(["api.v1", "playback.queue-events.v1"]),
				);
			const eventSourceFactory = vi.fn(() => ({
				addEventListener: vi.fn(),
				close: vi.fn(),
				onerror: null,
			}));
			const client = createApiClient({
				baseUrl: "http://music.test",
				transport,
				eventSourceFactory,
			});

			const unsubscribe = client.subscribePlaybackQueueEvents(vi.fn(), vi.fn());
			await Promise.resolve();
			await vi.advanceTimersByTimeAsync(999);
			expect(transport).toHaveBeenCalledOnce();
			await vi.advanceTimersByTimeAsync(1);
			expect(transport).toHaveBeenCalledTimes(2);
			await vi.advanceTimersByTimeAsync(1999);
			expect(transport).toHaveBeenCalledTimes(2);
			await vi.advanceTimersByTimeAsync(1);

			expect(transport).toHaveBeenCalledTimes(3);
			expect(eventSourceFactory).toHaveBeenCalledOnce();
			unsubscribe();
		} finally {
			vi.useRealTimers();
		}
	});

	it("does not subscribe when Queue events capability is absent", async () => {
		vi.useFakeTimers();
		try {
			const transport = vi
				.fn<typeof fetch>()
				.mockResolvedValue(serverHealthResponse(["api.v1"]));
			const eventSourceFactory = vi.fn();
			const onError = vi.fn();
			const client = createApiClient({
				baseUrl: "http://music.test",
				transport,
				eventSourceFactory,
			});

			client.subscribePlaybackQueueEvents(vi.fn(), onError);
			await vi.advanceTimersByTimeAsync(0);
			await vi.advanceTimersByTimeAsync(120000);

			expect(onError).toHaveBeenCalledOnce();
			expect(onError).toHaveBeenCalledWith(
				new Error(
					"Music Server does not advertise playback.queue-events.v1; Queue synchronization is disabled.",
				),
			);
			expect(transport).toHaveBeenCalledOnce();
			expect(eventSourceFactory).not.toHaveBeenCalled();
		} finally {
			vi.useRealTimers();
		}
	});

	it("cancels a pending capability retry when unsubscribed", async () => {
		vi.useFakeTimers();
		try {
			const transport = vi
				.fn<typeof fetch>()
				.mockRejectedValue(new Error("temporary health failure"));
			const eventSourceFactory = vi.fn();
			const client = createApiClient({
				baseUrl: "http://music.test",
				transport,
				eventSourceFactory,
			});

			const unsubscribe = client.subscribePlaybackQueueEvents(vi.fn(), vi.fn());
			await vi.advanceTimersByTimeAsync(0);
			unsubscribe();
			await vi.advanceTimersByTimeAsync(120000);

			expect(transport).toHaveBeenCalledOnce();
			expect(eventSourceFactory).not.toHaveBeenCalled();
		} finally {
			vi.useRealTimers();
		}
	});

	it("authenticates Queue event streams when a token is configured", async () => {
		const transport = vi
			.fn<typeof fetch>()
			.mockImplementation(async (input) => {
				if (input.toString().endsWith("/api/v1/health")) {
					return serverHealthResponse(["api.v1", "playback.queue-events.v1"]);
				}
				return new Response(
					'id: 8\nevent: queue-invalidated\ndata: {"revision":"opaque-8","sequence":"8","invalidates":["queue"]}\n\n',
					{ status: 200, headers: { "Content-Type": "text/event-stream" } },
				);
			});
		const client = createApiClient({
			baseUrl: "http://music.test",
			getToken: () => "secret-token",
			transport,
		});
		const onEvent = vi.fn();

		const unsubscribe = client.subscribePlaybackQueueEvents(onEvent);
		await vi.waitFor(() => expect(onEvent).toHaveBeenCalled());
		unsubscribe();

		const eventRequest = transport.mock.calls.find(([input]) =>
			input.toString().endsWith("/api/v1/playback/queue/events"),
		);
		const headers = new Headers(eventRequest?.[1]?.headers);
		expect(headers.get("Authorization")).toBe("Bearer secret-token");
		expect(onEvent).toHaveBeenCalledWith({
			revision: "opaque-8",
			sequence: "8",
			invalidates: ["queue"],
		});
	});
	it("builds health URL from base", () => {
		const client = createApiClient({ baseUrl: "http://localhost:8080" });
		expect(client.getHealth).toBeTypeOf("function");
		expect(client.getPreferences).toBeTypeOf("function");
		expect(client.listAlbums).toBeTypeOf("function");
		expect(client.getPlaybackQueue).toBeTypeOf("function");
		expect(client.searchLibrary).toBeTypeOf("function");
		expect(client.listPlaylists).toBeTypeOf("function");
		expect(client.createPlaylist).toBeTypeOf("function");
		expect(client.addPlaylistTrack).toBeTypeOf("function");
		expect(client.removePlaylistTrack).toBeTypeOf("function");
		expect(client.deleteTrack).toBeTypeOf("function");
		expect(client.previewTrackDeletion).toBeTypeOf("function");
		expect(client.createTrackReplacement).toBeTypeOf("function");
		expect(client.confirmTrackReplacement).toBeTypeOf("function");
		expect(client.listRadioStations).toBeTypeOf("function");
		expect(client.getRadioStation).toBeTypeOf("function");
		expect(client.searchRadioStations).toBeTypeOf("function");
		expect(client.getTrackStreamUrl("abc")).toBe(
			"http://localhost:8080/api/v1/tracks/abc/stream",
		);
		expect(
			client.getRadioStationStreamUrl("00000000-0000-4000-8000-000000000008"),
		).toBe(
			"http://localhost:8080/api/v1/radio/stations/00000000-0000-4000-8000-000000000008/stream",
		);
	});

	it("uses an injected transport for desktop requests", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			new Response(
				JSON.stringify({
					status: "ok",
					version: "0.1.0",
					capabilities: ["api.v1"],
					dependencies: [],
				}),
				{ status: 200 },
			),
		);
		const client = createApiClient({
			baseUrl: "http://127.0.0.1:8090",
			transport,
		});

		await expect(client.getHealth()).resolves.toMatchObject({ status: "ok" });
		expect(transport).toHaveBeenCalledWith(
			"http://127.0.0.1:8090/api/v1/health",
			expect.objectContaining({ headers: expect.any(Headers) }),
		);
	});

	it("builds media URLs from a separate native protocol base", () => {
		let mediaBaseUrl = "http://127.0.0.1:41000/token-1";
		const client = createApiClient({
			baseUrl: "",
			mediaBaseUrl: "earthly-media://localhost",
			streamBaseUrl: () => mediaBaseUrl,
		});

		expect(
			client.getAlbumCoverUrl("00000000-0000-4000-8000-000000000004"),
		).toBe(
			"earthly-media://localhost/api/v1/library/albums/00000000-0000-4000-8000-000000000004/cover",
		);
		// A size asks the server for a scaled-down cover.
		expect(
			client.getAlbumCoverUrl("00000000-0000-4000-8000-000000000004", 512),
		).toBe(
			"earthly-media://localhost/api/v1/library/albums/00000000-0000-4000-8000-000000000004/cover?size=512",
		);
		expect(
			client.getArtistPhotoUrl("00000000-0000-4000-8000-000000000005"),
		).toBe(
			"earthly-media://localhost/api/v1/library/artists/00000000-0000-4000-8000-000000000005/photo",
		);
		expect(
			client.getTrackStreamUrl("00000000-0000-4000-8000-000000000001"),
		).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/tracks/00000000-0000-4000-8000-000000000001/stream",
		);
		expect(
			client.getRadioStationStreamUrl("00000000-0000-4000-8000-000000000008"),
		).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/stations/00000000-0000-4000-8000-000000000008/stream",
		);
		expect(
			client.getRadioCatalogPreviewStreamUrl(
				"00000000-0000-4000-8000-000000000015",
			),
		).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/preview/00000000-0000-4000-8000-000000000015/stream",
		);

		mediaBaseUrl = "http://127.0.0.1:42000/token-2";
		expect(
			client.getTrackStreamUrl("00000000-0000-4000-8000-000000000002"),
		).toBe(
			"http://127.0.0.1:42000/token-2/api/v1/tracks/00000000-0000-4000-8000-000000000002/stream",
		);
	});

	it("sends Queue Revisions with every mutation", async () => {
		const transport = vi.fn<typeof fetch>().mockImplementation(
			async () =>
				new Response(JSON.stringify({ items: [], revision: "2" }), {
					status: 200,
				}),
		);
		const client = createApiClient({ baseUrl: "", transport });

		await client.replacePlaybackQueue(
			["00000000-0000-4000-8000-000000000001"],
			"1",
		);
		await client.appendPlaybackQueueItem(
			"00000000-0000-4000-8000-000000000001",
			"1",
		);
		await client.removePlaybackQueueItem(
			"00000000-0000-4000-8000-000000000006",
			"1",
		);
		await client.reorderPlaybackQueue(
			["00000000-0000-4000-8000-000000000006"],
			"1",
		);

		expect(JSON.parse(String(transport.mock.calls[0]?.[1]?.body))).toEqual({
			trackIds: ["00000000-0000-4000-8000-000000000001"],
			revision: "1",
		});
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			trackId: "00000000-0000-4000-8000-000000000001",
			revision: "1",
		});
		expect(
			new Headers(transport.mock.calls[2]?.[1]?.headers).get("If-Match"),
		).toBe("1");
		expect(JSON.parse(String(transport.mock.calls[3]?.[1]?.body))).toEqual({
			itemIds: ["00000000-0000-4000-8000-000000000006"],
			revision: "1",
		});
	});

	it("decodes conflict responses for every Queue mutation", async () => {
		const conflict = {
			error: "conflict",
			code: "queue_revision_conflict",
			message: "queue changed since supplied revision",
			queue: { items: [], revision: "2" },
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockImplementation(
				async () => new Response(JSON.stringify(conflict), { status: 409 }),
			);
		const client = createApiClient({ baseUrl: "", transport });
		const mutations = [
			() =>
				client.replacePlaybackQueue(
					["00000000-0000-4000-8000-000000000001"],
					"1",
				),
			() =>
				client.appendPlaybackQueueItem(
					"00000000-0000-4000-8000-000000000001",
					"1",
				),
			() => client.reorderPlaybackQueue([], "1"),
			() =>
				client.removePlaybackQueueItem(
					"00000000-0000-4000-8000-000000000006",
					"1",
				),
		];

		for (const mutation of mutations) {
			await expect(mutation()).rejects.toMatchObject({
				status: 409,
				body: conflict,
			});
		}
	});
});

function serverHealthResponse(capabilities: string[]) {
	return new Response(
		JSON.stringify({
			status: "ok",
			version: "0.1.0",
			capabilities,
			dependencies: [],
		}),
		{
			status: 200,
			headers: { "Content-Type": "application/json" },
		},
	);
}
