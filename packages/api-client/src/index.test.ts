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
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValue(
				new Response(
					JSON.stringify({ trackId: "track-1", peaks: [128], peakCount: 1 }),
				),
			);
		const client = createApiClient({ baseUrl: "http://music.test", transport });
		await client.getTrackWaveform("track-1");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/library/tracks/track-1/waveform",
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

		await expect(client.getArtistBiography("artist-1")).resolves.toEqual({
			text: "A band.",
			sourceUrl: "https://w.example/a",
		});
		await expect(client.getArtistBiography("artist-1")).resolves.toBeNull();
		await expect(client.getArtistBiography("artist-1")).rejects.toThrow("boom");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/library/artists/artist-1/biography",
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

		await expect(client.headTrackStream("track-1")).resolves.toEqual({
			status: 404,
		});
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/tracks/track-1/stream",
			expect.objectContaining({ method: "HEAD" }),
		);
		const headers = new Headers(transport.mock.calls[0]?.[1]?.headers);
		expect(headers.get("Authorization")).toBe("Bearer secret");
	});

	it("searches the External Source and resolves an External Track's stream URL", async () => {
		type ExternalTrack = components["schemas"]["ExternalTrack"];
		expectTypeOf<ExternalTrack["artistName"]>().toEqualTypeOf<string | null>();
		expectTypeOf<ExternalTrack["durationMs"]>().toEqualTypeOf<number | null>();
		expectTypeOf<
			components["schemas"]["ExternalTrackPlayback"]["format"]
		>().toEqualTypeOf<"progressive" | "hls">();
		const track: ExternalTrack = {
			id: "eyJ1Ijoi.c2ln",
			title: "One More Time",
			artistName: "Daft Punk",
			durationMs: 320_000,
			webpageUrl: "https://soundcloud.com/daftpunk/one-more-time",
			extractor: "Soundcloud",
		};
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValueOnce(
				Response.json({ source: "scsearch", items: [track] }),
			)
			.mockResolvedValueOnce(Response.json({ source: "scsearch", items: [] }))
			.mockResolvedValueOnce(
				Response.json({
					format: "hls",
					streamPath: `/api/v1/external/tracks/${track.id}/playlist.m3u8`,
				}),
			);
		const client = createApiClient({
			baseUrl: "http://music.test",
			streamBaseUrl: "http://127.0.0.1:43129/token",
			transport,
		});

		await expect(
			client.searchExternalSource("daft punk & co"),
		).resolves.toEqual({ source: "scsearch", items: [track] });
		await client.searchExternalSource("daft punk", 5);
		const playback = await client.getExternalTrackPlayback(track.id);
		expect(transport.mock.calls.map(([input]) => input)).toEqual([
			"http://music.test/api/v1/external/search?q=daft+punk+%26+co",
			"http://music.test/api/v1/external/search?q=daft+punk&limit=5",
			"http://music.test/api/v1/external/tracks/eyJ1Ijoi.c2ln/playback",
		]);
		expect(client.getExternalStreamUrl(playback.streamPath)).toBe(
			"http://127.0.0.1:43129/token/api/v1/external/tracks/eyJ1Ijoi.c2ln/playlist.m3u8",
		);
	});

	it("previews and explicitly confirms Permanent Track Deletion", async () => {
		const preview = {
			trackId: "track-1",
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

		await expect(client.previewTrackDeletion("track-1")).resolves.toEqual(
			preview,
		);
		await expect(client.deleteTrack("track-1", "token-1")).resolves.toEqual({
			deletedFiles: 1,
		});

		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/library/tracks/track-1",
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
			albumId: "album-1",
			albumTitle: "1989",
			trackCount: 2,
			totalSizeBytes: 22,
			tracks: [
				{ trackId: "track-1", trackTitle: "First", sizeBytes: 11 },
				{ trackId: "track-2", trackTitle: "Second", sizeBytes: 11 },
			],
			playlistReferences: [],
			queueReferences: [{ userId: "user-1", itemCount: 2 }],
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

		await expect(client.previewAlbumDeletion("album-1")).resolves.toEqual(
			preview,
		);
		await expect(client.deleteAlbum("album-1", "album-token")).resolves.toEqual(
			result,
		);

		expect(transport).toHaveBeenNthCalledWith(
			1,
			"http://music.test/api/v1/library/albums/album-1/deletion",
			expect.anything(),
		);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/library/albums/album-1",
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
			id: "batch-1",
			status: "uploading",
			revision: 1,
			files: [],
		};
		const completedBatch = {
			id: "batch-1",
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
						id: "import-1",
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
			["import-1"],
			[{ jobId: "import-1", action: "import_separately" }],
		);

		expect(report.status).toBe("completed");
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			batchId: "batch-1",
			clientFileId: "00000000-0000-4000-8000-000000000001",
		});
		expect(JSON.parse(String(transport.mock.calls[2]?.[1]?.body))).toEqual({
			revision: 2,
			selectedFileIds: ["import-1"],
			duplicateDecisions: [{ jobId: "import-1", action: "import_separately" }],
		});
	});

	it("polls observable Managed Import validation results", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			Response.json({
				id: "import-1",
				status: "failed",
				revision: 1,
				validationProgress: 40,
				errorCode: "validation_cancelled",
			}),
		);
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		const result = await client.getManagedImportJob("import-1");

		expect(result.validationProgress).toBe(40);
		expect(result.errorCode).toBe("validation_cancelled");
		expect(transport).toHaveBeenCalledWith(
			"http://music.test/api/v1/imports/import-1",
			expect.objectContaining({ headers: expect.any(Headers) }),
		);
	});

	it("cancels Managed Import staging through explicit DELETE requests", async () => {
		const transport = vi
			.fn<typeof fetch>()
			.mockResolvedValue(new Response(null, { status: 204 }));
		const client = createApiClient({ baseUrl: "http://music.test", transport });

		await client.cancelManagedImportBatch("batch-1");
		await client.cancelManagedImport("import-1");

		expect(transport).toHaveBeenNthCalledWith(
			1,
			"http://music.test/api/v1/import-batches/batch-1",
			expect.objectContaining({ method: "DELETE" }),
		);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/imports/import-1",
			expect.objectContaining({ method: "DELETE" }),
		);
	});

	it("lists retained terminal Import History results", async () => {
		const payload = {
			items: [
				{
					importId: "batch-1",
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
							fileId: "file-1",
							jobId: "job-1",
							safeFilename: "track.flac",
							startedAt: "2026-09-02T10:00:00Z",
							completedAt: "2026-09-02T10:01:00Z",
							contentSha256: "0".repeat(64),
							resultCode: "imported",
							createdTrackId: "track-1",
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
						id: "import-1",
						status: "uploading",
						revision: 1,
						validationProgress: 0,
					},
					{ status: 201 },
				),
			)
			.mockResolvedValueOnce(
				Response.json({
					jobId: "import-1",
					status: "awaiting_confirmation",
					revision: 2,
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
					jobId: "import-1",
					status: "committed",
					revision: 3,
					trackId: "track-1",
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

		expect(result.trackId).toBe("track-1");
		expect(progress).toEqual([0, 100]);
		expect(transport).toHaveBeenNthCalledWith(
			2,
			"http://music.test/api/v1/imports/import-1/file",
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
				jobId: "import-1",
				status: "awaiting_confirmation",
				revision: 2,
			}),
		);
		const client = createApiClient({
			baseUrl: "http://music.test",
			transport,
		});

		await client.uploadManagedImportFile(
			"import-1",
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
				"import-1",
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
					id: "album-1",
					title: "Deprecated-shape Album",
					artistId: "artist-1",
					artistName: "Deprecated-shape Artist",
					genres: ["Rock"],
					tracks: [
						{
							id: "track-1",
							title: "Deprecated-shape Track",
							artistName: "Track Artist",
							albumId: "album-1",
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
						id: "track-1",
						title: "Deprecated-shape Track",
						artistName: "Track Artist",
						albumId: "album-1",
						durationMs: 1000,
						format: "flac",
						genre: "Electronic / Ambient",
					},
				],
				total: 1,
			});
		});
		const client = createApiClient({ baseUrl: "", transport });

		const album = await client.getAlbum("album-1");
		const tracks = await client.listTracks();

		expect(album.albumArtists).toEqual([
			{ id: "artist-1", name: "Deprecated-shape Artist" },
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
		expect(client.getRadioStationStreamUrl("station-1")).toBe(
			"http://localhost:8080/api/v1/radio/stations/station-1/stream",
		);
	});

	it("uses an injected transport for desktop requests", async () => {
		const transport = vi.fn<typeof fetch>().mockResolvedValue(
			new Response(
				JSON.stringify({
					status: "ok",
					version: "0.1.0",
					capabilities: ["api.v1"],
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

		expect(client.getAlbumCoverUrl("album-1")).toBe(
			"earthly-media://localhost/api/v1/library/albums/album-1/cover",
		);
		// A size asks the server for a scaled-down cover.
		expect(client.getAlbumCoverUrl("album-1", 512)).toBe(
			"earthly-media://localhost/api/v1/library/albums/album-1/cover?size=512",
		);
		expect(client.getArtistPhotoUrl("artist-1")).toBe(
			"earthly-media://localhost/api/v1/library/artists/artist-1/photo",
		);
		expect(client.getTrackStreamUrl("track-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/tracks/track-1/stream",
		);
		expect(client.getRadioStationStreamUrl("station-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/stations/station-1/stream",
		);
		expect(client.getRadioCatalogPreviewStreamUrl("preview-1")).toBe(
			"http://127.0.0.1:41000/token-1/api/v1/radio/preview/preview-1/stream",
		);

		mediaBaseUrl = "http://127.0.0.1:42000/token-2";
		expect(client.getTrackStreamUrl("track-2")).toBe(
			"http://127.0.0.1:42000/token-2/api/v1/tracks/track-2/stream",
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

		await client.replacePlaybackQueue(["track-1"], "1");
		await client.appendPlaybackQueueItem("track-1", "1");
		await client.removePlaybackQueueItem("item-1", "1");
		await client.reorderPlaybackQueue(["item-1"], "1");

		expect(JSON.parse(String(transport.mock.calls[0]?.[1]?.body))).toEqual({
			trackIds: ["track-1"],
			revision: "1",
		});
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			trackId: "track-1",
			revision: "1",
		});
		expect(
			new Headers(transport.mock.calls[2]?.[1]?.headers).get("If-Match"),
		).toBe("1");
		expect(JSON.parse(String(transport.mock.calls[3]?.[1]?.body))).toEqual({
			itemIds: ["item-1"],
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
			() => client.replacePlaybackQueue(["track-1"], "1"),
			() => client.appendPlaybackQueueItem("track-1", "1"),
			() => client.reorderPlaybackQueue([], "1"),
			() => client.removePlaybackQueueItem("item-1", "1"),
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
		JSON.stringify({ status: "ok", version: "0.1.0", capabilities }),
		{
			status: 200,
			headers: { "Content-Type": "application/json" },
		},
	);
}
