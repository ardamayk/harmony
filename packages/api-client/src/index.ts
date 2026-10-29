import type { components, operations } from "./generated/schema";
import {
	type QueueEvent,
	type QueueEventSource,
	subscribeQueueEvents,
} from "./queue-events";
import {
	type ApiOperation,
	isDevelopmentBuild,
	validateResponse,
} from "./response-validation";

/**
 * Longest-edge sizes the server scales covers to. Pick the one at or above
 * the displayed size times the device pixel ratio (2x).
 */
export type CoverSize = 64 | 128 | 256 | 512 | 1024;

export * from "./capabilities";
export type { QueueEvent, QueueEventSource } from "./queue-events";
export { QUEUE_EVENTS_CAPABILITY } from "./queue-events";
export {
	type ApiOperation,
	ApiResponseValidationError,
} from "./response-validation";

export type ApiClientConfig = {
	baseUrl: string;
	mediaBaseUrl?: string | (() => string);
	streamBaseUrl?: string | (() => string);
	queueEventsBaseUrl?: string | (() => string | Promise<string>);
	getToken?: () => string | undefined;
	transport?: typeof fetch;
	eventSourceFactory?: (url: string) => QueueEventSource;
	queueEventSubscriber?: (
		onEvent: (event: QueueEvent) => void,
		onError: (error: Error) => void,
	) => (() => void) | Promise<() => void>;
	/**
	 * Checks every JSON response against the operation's schema from the
	 * contract and throws ApiResponseValidationError on a mismatch. For
	 * development and tests only: production builds must leave it off, since
	 * library lists can hold thousands of items. Defaults to on when
	 * `process.env.NODE_ENV` is set to anything but "production" (Node test
	 * runs), off otherwise.
	 */
	validateResponses?: boolean;
};

type Schemas = components["schemas"];
type WireAlbum = Schemas["Album"];
type WireAlbumDetail = Schemas["AlbumDetail"];
type WireAlbumList = Schemas["AlbumList"];
type WireTrack = Schemas["Track"];
type WireTrackList = Schemas["TrackList"];
type WirePlaylistDetail = Schemas["PlaylistDetail"];
type WireQueue = Schemas["Queue"];
type WireQueueItem = Schemas["QueueItem"];

export type HealthResponse = Schemas["HealthResponse"];
export type ExternalSourceInfo = Schemas["ExternalSourceInfo"];
export type ThemePreferences = Schemas["ThemePreferences"];
export type LayoutPreferences = Schemas["LayoutPreferences"];
export type PlaybackPreferences = Schemas["PlaybackPreferences"];
export type PlaybackPreferencesPatch = Schemas["PlaybackPreferencesPatch"];
export type UserPreferences = Schemas["UserPreferences"];
export type UserPreferencesPatch = Schemas["UserPreferencesPatch"];
export type User = Schemas["User"];
export type Artist = Schemas["Artist"];
export type ArtistCredit = Schemas["ArtistCredit"];
export type Genre = Schemas["Genre"];
export type ReleaseIdentifier = Schemas["ReleaseIdentifier"];
export type AlbumArtwork = Schemas["AlbumArtwork"];
export type ArtistList = Schemas["ArtistList"];
export type ArtistBiography = Schemas["ArtistBiography"];
export type Album = Omit<
	WireAlbum,
	"albumArtists" | "genreItems" | "releaseIdentifiers"
> & {
	albumArtists: ArtistCredit[];
	genreItems: Genre[];
	releaseIdentifiers: ReleaseIdentifier[];
};
export type TrackLyrics = components["schemas"]["TrackLyrics"];
export type TrackWaveform = components["schemas"]["TrackWaveform"];
export type TrackWaveformPending =
	components["schemas"]["TrackWaveformPending"];
export type Track = Omit<WireTrack, "artists" | "discNo" | "genres"> & {
	artists: ArtistCredit[];
	discNo: number;
	genres: Genre[];
};
export type AlbumList = Omit<WireAlbumList, "items"> & { items: Album[] };
export type AlbumDetail = Omit<WireAlbumDetail, keyof Album | "tracks"> &
	Album & { tracks: Track[] };
export type TrackList = Omit<WireTrackList, "items"> & { items: Track[] };
export type LibrarySearchResponse = Schemas["LibrarySearchResponse"];
export type LibrarySearchResult = Schemas["LibrarySearchResult"];
export type LibrarySearchResultType = Schemas["LibrarySearchResultType"];
export type LibrarySearchMatch = Schemas["LibrarySearchMatch"];
export type Playlist = Schemas["Playlist"];
export type PlaylistList = Schemas["PlaylistList"];
export type PlaylistDetail = Omit<WirePlaylistDetail, "tracks"> & {
	tracks: Track[];
};
export type PlaylistCreate = Schemas["PlaylistCreate"];
export type PlaylistTrackAdd = Schemas["PlaylistTrackAdd"];
export type DeleteResult = Schemas["DeleteResult"];
export type TrackDeletionPreview = Schemas["TrackDeletionPreview"];
export type TrackReplacementPreview = Schemas["TrackReplacementPreview"];
export type TrackReplacementFieldDiff = Schemas["TrackReplacementFieldDiff"];
export type TrackReplacementResult = Schemas["TrackReplacementResult"];
export type QueueItemSource = Schemas["QueueItemSource"];
export type QueueItem = Omit<WireQueueItem, "track"> & { track: Track };
export type Queue = Omit<WireQueue, "items"> & { items: QueueItem[] };
export type ErrorResponse = Schemas["ErrorResponse"];
export type ManagedImportJob = Schemas["ManagedImportJob"];
export type ManagedImportAlbumDecision = Schemas["ManagedImportAlbumDecision"];
export type ManagedImportAlbumPreview = Schemas["ManagedImportAlbumPreview"];
export type ManagedImportArtworkOption = Schemas["ManagedImportArtworkOption"];
export type ManagedImportBatch = Schemas["ManagedImportBatch"];
export type ManagedImportBatchCreate = Schemas["ManagedImportBatchCreate"];
export type ManagedImportBatchFile = Schemas["ManagedImportBatchFile"];
export type ManagedImportDuplicateDecision =
	Schemas["ManagedImportDuplicateDecision"];
export type ManagedImportPreview = Schemas["ManagedImportPreview"];
export type ManagedImportPreviewFile = Schemas["ManagedImportPreviewFile"];
export type ManagedImportResult = Schemas["ManagedImportResult"];
export type ManagedImportHistoryList = Schemas["ManagedImportHistoryList"];
export type ManagedImportHistoryItem = Schemas["ManagedImportHistoryItem"];
export type ManagedImportHistoryFile = Schemas["ManagedImportHistoryFile"];
export type AlbumDeletionPreview = Schemas["AlbumDeletionPreview"];
export type AlbumDeletionResult = Schemas["AlbumDeletionResult"];
export type ManagedImportUploadProgress = (
	progress: number,
	transferredBytes?: number,
) => void;
export type QueueConflictResponse = Omit<
	Schemas["QueueConflictResponse"],
	"queue"
> & {
	queue: Queue;
};
export type QueueReplace = Schemas["QueueReplace"];
export type QueueItemAppend = Schemas["QueueItemAppend"];
export type QueueReorder = Schemas["QueueReorder"];
export type RadioNowPlaying = Schemas["RadioNowPlaying"];
export type RadioStation = Schemas["RadioStation"];
export type RadioStationList = Schemas["RadioStationList"];
export type RadioStationCreate = Schemas["RadioStationCreate"];
export type RadioStationPatch = Schemas["RadioStationPatch"];
export type RadioSearchResult = Schemas["RadioSearchResult"];
export type RadioSearchResultList = Schemas["RadioSearchResultList"];
export type RadioCatalogOption = Schemas["RadioCatalogOption"];
export type RadioCatalogOptionList = Schemas["RadioCatalogOptionList"];
export type RadioImportRequest = Schemas["RadioImportRequest"];
export type ExternalTrack = Schemas["ExternalTrack"];
export type ExternalSearchResponse = Schemas["ExternalSearchResponse"];
export type ExternalTrackPlayback = Schemas["ExternalTrackPlayback"];

export class ApiError extends Error {
	constructor(
		public readonly status: number,
		public readonly body: ErrorResponse | QueueConflictResponse,
	) {
		super(body.message);
		this.name = "ApiError";
	}
}

export type ListParams = NonNullable<
	operations["listAlbums"]["parameters"]["query"]
>;
export type RadioSearchParams = NonNullable<
	operations["searchRadioStations"]["parameters"]["query"]
>;

function buildQuery(params?: ListParams): string {
	if (!params) return "";
	const search = new URLSearchParams();
	if (params.limit != null) search.set("limit", String(params.limit));
	if (params.offset != null) search.set("offset", String(params.offset));
	if (params.q) search.set("q", params.q);
	if (params.artistId) search.set("artistId", params.artistId);
	const qs = search.toString();
	return qs ? `?${qs}` : "";
}

function buildRadioSearchQuery(params?: RadioSearchParams): string {
	if (!params) return "";
	const search = new URLSearchParams();
	if (params.q) search.set("q", params.q);
	if (params.country) search.set("country", params.country);
	if (params.language) search.set("language", params.language);
	if (params.tag) search.set("tag", params.tag);
	if (params.codec) search.set("codec", params.codec);
	if (params.codecGroup) search.set("codecGroup", params.codecGroup);
	if (params.minBitrate != null)
		search.set("minBitrate", String(params.minBitrate));
	if (params.limit != null) search.set("limit", String(params.limit));
	if (params.offset != null) search.set("offset", String(params.offset));
	const qs = search.toString();
	return qs ? `?${qs}` : "";
}

function legacyArtistCredit(name: string): ArtistCredit {
	return { id: `legacy-artist:${name}`, name };
}

function legacyGenre(name: string): Genre {
	return { id: `legacy-genre:${name}`, name };
}

function normalizeTrack(track: WireTrack): Track {
	return {
		...track,
		artists: track.artists ?? [legacyArtistCredit(track.artistName)],
		albumArtists: track.albumArtists ?? [],
		discNo: track.discNo ?? 1,
		genres: track.genres ?? (track.genre ? [legacyGenre(track.genre)] : []),
	};
}

function normalizeAlbum(album: WireAlbum): Album {
	return {
		...album,
		albumArtists: album.albumArtists ?? [
			{ id: album.artistId, name: album.artistName },
		],
		genreItems: album.genreItems ?? (album.genres ?? []).map(legacyGenre),
		releaseIdentifiers: album.releaseIdentifiers ?? [],
	};
}

function normalizeAlbumList(albums: WireAlbumList): AlbumList {
	return { ...albums, items: albums.items.map(normalizeAlbum) };
}

function normalizeAlbumDetail(album: WireAlbumDetail): AlbumDetail {
	return {
		...normalizeAlbum(album),
		tracks: album.tracks.map(normalizeTrack),
	};
}

function normalizeTrackList(tracks: WireTrackList): TrackList {
	return { ...tracks, items: tracks.items.map(normalizeTrack) };
}

function normalizePlaylistDetail(playlist: WirePlaylistDetail): PlaylistDetail {
	return { ...playlist, tracks: playlist.tracks.map(normalizeTrack) };
}

function normalizeQueue(queue: WireQueue): Queue {
	return {
		...queue,
		items: queue.items.map((item) => ({
			...item,
			track: normalizeTrack(item.track),
			source: item.source ?? { kind: "user" },
		})),
	};
}

function normalizeApiErrorBody(
	body: ErrorResponse | Schemas["QueueConflictResponse"],
): ErrorResponse | QueueConflictResponse {
	if ("queue" in body) {
		return { ...body, queue: normalizeQueue(body.queue) };
	}
	return body;
}

export function createApiClient(config: ApiClientConfig) {
	const {
		baseUrl,
		mediaBaseUrl = baseUrl,
		streamBaseUrl = mediaBaseUrl,
		queueEventsBaseUrl = baseUrl,
		getToken,
		transport = globalThis.fetch,
		eventSourceFactory = (url) => new EventSource(url) as QueueEventSource,
		queueEventSubscriber,
		validateResponses = isDevelopmentBuild(),
	} = config;
	const getMediaBaseUrl = () =>
		typeof mediaBaseUrl === "function" ? mediaBaseUrl() : mediaBaseUrl;
	const getStreamBaseUrl = () =>
		typeof streamBaseUrl === "function" ? streamBaseUrl() : streamBaseUrl;
	const getQueueEventsBaseUrl = () =>
		typeof queueEventsBaseUrl === "function"
			? queueEventsBaseUrl()
			: queueEventsBaseUrl;

	function uploadManagedImportFile(
		importId: string,
		originalFilename: string,
		file: Blob,
		onProgress?: ManagedImportUploadProgress,
		signal?: AbortSignal,
	): Promise<ManagedImportPreview> {
		if (config.transport || typeof XMLHttpRequest === "undefined") {
			onProgress?.(0);
			return request<ManagedImportPreview>(
				"uploadManagedImportFile",
				`/api/v1/imports/${importId}/file`,
				{
					method: "PUT",
					headers: {
						"Content-Type": file.type || "application/octet-stream",
						"X-Import-Filename": originalFilename,
					},
					body: file,
					signal,
				},
			).then((preview) => {
				onProgress?.(100);
				return preview;
			});
		}
		return new Promise((resolve, reject) => {
			const upload = new XMLHttpRequest();
			const removeAbortListener = () =>
				signal?.removeEventListener("abort", handleSignalAbort);
			const handleSignalAbort = () => upload.abort();
			upload.open("PUT", `${baseUrl}/api/v1/imports/${importId}/file`);
			upload.responseType = "json";
			upload.setRequestHeader("Accept", "application/json");
			upload.setRequestHeader(
				"Content-Type",
				file.type || "application/octet-stream",
			);
			upload.setRequestHeader("X-Import-Filename", originalFilename);
			const token = getToken?.();
			if (token) upload.setRequestHeader("Authorization", `Bearer ${token}`);
			upload.upload.addEventListener("progress", (event) => {
				if (event.lengthComputable && event.total > 0) {
					onProgress?.(
						Math.round((event.loaded / event.total) * 100),
						event.loaded,
					);
				}
			});
			upload.addEventListener("load", async () => {
				removeAbortListener();
				if (upload.status >= 200 && upload.status < 300) {
					try {
						if (validateResponses) {
							await validateResponse(
								"uploadManagedImportFile",
								upload.status,
								upload.response,
							);
						}
					} catch (error) {
						reject(error);
						return;
					}
					onProgress?.(100);
					resolve(upload.response as ManagedImportPreview);
					return;
				}
				const body = upload.response as ErrorResponse | undefined;
				reject(
					body
						? new ApiError(upload.status, body)
						: new Error(`HTTP ${upload.status}`),
				);
			});
			upload.addEventListener("error", () => {
				removeAbortListener();
				reject(new Error("Managed Import upload failed"));
			});
			upload.addEventListener("abort", () => {
				removeAbortListener();
				reject(
					new DOMException("Managed Import upload canceled", "AbortError"),
				);
			});
			signal?.addEventListener("abort", handleSignalAbort, { once: true });
			if (signal?.aborted) {
				removeAbortListener();
				reject(
					new DOMException("Managed Import upload canceled", "AbortError"),
				);
				return;
			}
			onProgress?.(0);
			upload.send(file);
		});
	}

	function subscribePlaybackQueueEvents(
		onEvent: (event: QueueEvent) => void,
		onError?: (error: Error) => void,
	): () => void {
		return subscribeQueueEvents(
			{
				getBaseUrl: getQueueEventsBaseUrl,
				getCapabilities: async () =>
					(await request<HealthResponse>("getHealth", "/api/v1/health"))
						.capabilities,
				getToken,
				transport,
				eventSourceFactory,
				subscriber: queueEventSubscriber,
			},
			onEvent,
			onError,
		);
	}

	async function request<T>(
		operation: ApiOperation,
		path: string,
		init?: RequestInit,
	): Promise<T> {
		const headers = new Headers(init?.headers);
		headers.set("Accept", "application/json");
		if (init?.body && !headers.has("Content-Type")) {
			headers.set("Content-Type", "application/json");
		}
		const token = getToken?.();
		if (token) {
			headers.set("Authorization", `Bearer ${token}`);
		}

		const response = await transport(`${baseUrl}${path}`, {
			...init,
			headers,
		});

		if (!response.ok) {
			const body = normalizeApiErrorBody(
				(await response.json().catch(() => ({
					error: "unknown",
					code: "unknown",
					message: response.statusText,
				}))) as ErrorResponse | Schemas["QueueConflictResponse"],
			);
			throw new ApiError(response.status, body);
		}

		if (response.status === 204) {
			return undefined as T;
		}

		const body: unknown = await response.json();
		if (validateResponses) {
			await validateResponse(operation, response.status, body);
		}
		return body as T;
	}

	return {
		getHealth: () => request<HealthResponse>("getHealth", "/api/v1/health"),
		getMe: () => request<User>("getMe", "/api/v1/me"),
		getPreferences: () =>
			request<UserPreferences>("getPreferences", "/api/v1/preferences"),
		patchPreferences: (body: UserPreferencesPatch) =>
			request<UserPreferences>("patchPreferences", "/api/v1/preferences", {
				method: "PATCH",
				body: JSON.stringify(body),
			}),

		listArtists: (params?: ListParams) =>
			request<ArtistList>(
				"listArtists",
				`/api/v1/library/artists${buildQuery(params)}`,
			),
		getArtist: (artistId: string) =>
			request<Artist>("getArtist", `/api/v1/library/artists/${artistId}`),
		/** Resolves `null` when the Artist has no biography (yet). */
		getArtistBiography: (artistId: string) =>
			request<ArtistBiography>(
				"getArtistBiography",
				`/api/v1/library/artists/${artistId}/biography`,
			).catch((error: unknown) => {
				if (error instanceof ApiError && error.status === 404) return null;
				throw error;
			}),
		/** Last.fm play counts by Track id; empty without a server key. */
		listTrackPlayCounts: (trackIds: string[]) =>
			trackIds.length === 0
				? Promise.resolve(new Map<string, number>())
				: request<Schemas["PlayCountList"]>(
						"listTrackPlayCounts",
						`/api/v1/library/tracks/play-counts?ids=${encodeURIComponent(trackIds.join(","))}`,
					).then(
						(list) =>
							new Map(list.items.map((item) => [item.trackId, item.playCount])),
					),
		/** Deezer's genres for an Album whose tags name none (ADR 0027). */
		listAlbumOnlineGenres: (albumId: string) =>
			request<Schemas["GenreNameList"]>(
				"listAlbumOnlineGenres",
				`/api/v1/library/albums/${albumId}/online-genres`,
			).then((list) => list.items),
		listArtistPopularTracks: (artistId: string, limit?: number) =>
			request<Schemas["PopularTrackList"]>(
				"listArtistPopularTracks",
				`/api/v1/library/artists/${artistId}/popular-tracks${limit ? `?limit=${limit}` : ""}`,
			).then((list) => list.items.map(normalizeTrack)),
		listAlbums: (params?: ListParams) =>
			request<WireAlbumList>(
				"listAlbums",
				`/api/v1/library/albums${buildQuery(params)}`,
			).then(normalizeAlbumList),
		getAlbum: (albumId: string) =>
			request<WireAlbumDetail>(
				"getAlbum",
				`/api/v1/library/albums/${albumId}`,
			).then(normalizeAlbumDetail),
		listTracks: (params?: ListParams) =>
			request<WireTrackList>(
				"listTracks",
				`/api/v1/library/tracks${buildQuery(params)}`,
			).then(normalizeTrackList),
		getTrack: (trackId: string) =>
			request<WireTrack>("getTrack", `/api/v1/library/tracks/${trackId}`).then(
				normalizeTrack,
			),
		getTrackLyrics: (trackId: string) =>
			request<TrackLyrics>(
				"getTrackLyrics",
				`/api/v1/library/tracks/${trackId}/lyrics`,
			),
		/** Resolves the pending shape (HTTP 202) while peaks are still being generated. */
		getTrackWaveform: (trackId: string) =>
			request<TrackWaveform | TrackWaveformPending>(
				"getTrackWaveform",
				`/api/v1/library/tracks/${trackId}/waveform`,
				{
					cache: "no-cache",
				},
			),
		previewAlbumDeletion: (albumId: string) =>
			request<AlbumDeletionPreview>(
				"previewAlbumDeletion",
				`/api/v1/library/albums/${albumId}/deletion`,
			),
		deleteAlbum: (albumId: string, confirmationToken: string) =>
			request<AlbumDeletionResult>(
				"deleteAlbum",
				`/api/v1/library/albums/${albumId}`,
				{
					method: "DELETE",
					headers: { "X-Permanent-Delete": "1" },
					body: JSON.stringify({ confirmationToken }),
				},
			),
		previewTrackDeletion: (trackId: string) =>
			request<TrackDeletionPreview>(
				"previewTrackDeletion",
				`/api/v1/library/tracks/${trackId}/deletion`,
			),
		deleteTrack: (trackId: string, confirmationToken: string) =>
			request<DeleteResult>(
				"deleteTrack",
				`/api/v1/library/tracks/${trackId}`,
				{
					method: "DELETE",
					headers: { "X-Permanent-Delete": "1" },
					body: JSON.stringify({ confirmationToken }),
				},
			),
		createTrackReplacement: (trackId: string) =>
			request<ManagedImportJob>(
				"createTrackReplacement",
				`/api/v1/library/tracks/${trackId}/replacement`,
				{
					method: "POST",
				},
			),
		confirmTrackReplacement: (
			importId: string,
			revision: number,
			confirmationToken: string,
		) =>
			request<TrackReplacementResult>(
				"confirmTrackReplacement",
				`/api/v1/imports/${importId}/replacement`,
				{
					method: "POST",
					headers: { "X-Track-Replacement": "1" },
					body: JSON.stringify({ revision, confirmationToken }),
				},
			),
		listImportHistory: () =>
			request<ManagedImportHistoryList>(
				"listImportHistory",
				"/api/v1/import-history",
			),
		createManagedImportBatch: (options?: ManagedImportBatchCreate) =>
			request<ManagedImportBatch>(
				"createManagedImportBatch",
				"/api/v1/import-batches",
				{
					method: "POST",
					body: options ? JSON.stringify(options) : undefined,
				},
			),
		uploadManagedImportArtwork: (
			batchId: string,
			albumKey: string,
			file: File,
		) =>
			request<ManagedImportArtworkOption>(
				"uploadManagedImportArtwork",
				`/api/v1/import-batches/${batchId}/albums/${albumKey}/artwork`,
				{
					method: "PUT",
					body: file,
					headers: { "Content-Type": "application/octet-stream" },
				},
			),
		getManagedImportArtworkUrl: (batchId: string, artworkId: string) =>
			`${getMediaBaseUrl()}/api/v1/import-batches/${batchId}/artwork/${encodeURIComponent(artworkId)}`,
		heartbeatManagedImportBatch: (batchId: string) =>
			request<void>(
				"heartbeatManagedImportBatch",
				`/api/v1/import-batches/${batchId}/heartbeat`,
				{
					method: "POST",
				},
			),
		getManagedImportBatch: (batchId: string) =>
			request<ManagedImportBatch>(
				"getManagedImportBatch",
				`/api/v1/import-batches/${batchId}`,
			),
		cancelManagedImportBatch: (batchId: string, keepalive = false) =>
			request<void>(
				"cancelManagedImportBatch",
				`/api/v1/import-batches/${batchId}`,
				{
					method: "DELETE",
					keepalive,
				},
			),
		confirmManagedImportBatch: (
			batchId: string,
			revision: number,
			selectedFileIds: string[],
			duplicateDecisions?: ManagedImportDuplicateDecision[],
			albumDecisions?: ManagedImportAlbumDecision[],
		) =>
			request<ManagedImportBatch>(
				"confirmManagedImportBatch",
				`/api/v1/import-batches/${batchId}/confirm`,
				{
					method: "POST",
					body: JSON.stringify({
						revision,
						selectedFileIds,
						duplicateDecisions,
						albumDecisions,
					}),
				},
			),
		createManagedImportJob: (batchId?: string, clientFileId?: string) =>
			request<ManagedImportJob>("createManagedImportJob", "/api/v1/imports", {
				method: "POST",
				body: batchId ? JSON.stringify({ batchId, clientFileId }) : undefined,
			}),
		getManagedImportJob: (importId: string) =>
			request<ManagedImportJob>(
				"getManagedImportJob",
				`/api/v1/imports/${importId}`,
			),
		cancelManagedImport: (importId: string) =>
			request<void>("cancelManagedImport", `/api/v1/imports/${importId}`, {
				method: "DELETE",
			}),
		uploadManagedImportFile,
		confirmManagedImport: (
			importId: string,
			revision: number,
			duplicateDecision?: ManagedImportDuplicateDecision["action"],
		) =>
			request<ManagedImportResult>(
				"confirmManagedImport",
				`/api/v1/imports/${importId}/confirm`,
				{
					method: "POST",
					body: JSON.stringify({ revision, duplicateDecision }),
				},
			),

		getPlaybackQueue: () =>
			request<WireQueue>("getPlaybackQueue", "/api/v1/playback/queue").then(
				normalizeQueue,
			),
		subscribePlaybackQueueEvents,
		replacePlaybackQueue: (
			trackIds: string[],
			revision: string,
			source?: QueueItemSource,
		) =>
			request<WireQueue>("replacePlaybackQueue", "/api/v1/playback/queue", {
				method: "PUT",
				body: JSON.stringify({ trackIds, revision, source }),
			}).then(normalizeQueue),
		reorderPlaybackQueue: (itemIds: string[], revision: string) =>
			request<WireQueue>("reorderPlaybackQueue", "/api/v1/playback/queue", {
				method: "PATCH",
				body: JSON.stringify({ itemIds, revision }),
			}).then(normalizeQueue),
		appendPlaybackQueueItem: (
			trackId: string,
			revision: string,
			source?: QueueItemSource,
		) =>
			request<WireQueue>(
				"appendPlaybackQueueItem",
				"/api/v1/playback/queue/items",
				{
					method: "POST",
					body: JSON.stringify({ trackId, revision, source }),
				},
			).then(normalizeQueue),
		removePlaybackQueueItem: (itemId: string, revision: string) =>
			request<WireQueue>(
				"removePlaybackQueueItem",
				`/api/v1/playback/queue/items/${itemId}`,
				{
					method: "DELETE",
					headers: { "If-Match": revision },
				},
			).then(normalizeQueue),
		/** Removes the items after afterItemId and keeps the rest; null clears the whole queue. */
		clearPlaybackQueue: (afterItemId: string | null, revision: string) =>
			request<WireQueue>(
				"clearPlaybackQueue",
				`/api/v1/playback/queue${afterItemId ? `?${new URLSearchParams({ afterItemId }).toString()}` : ""}`,
				{ method: "DELETE", headers: { "If-Match": revision } },
			).then(normalizeQueue),
		/** One coherent Library Search; the server ranks, gates typo correction, and picks the Best Match. */
		searchLibrary: (q: string, all = false) =>
			request<LibrarySearchResponse>(
				"searchLibrary",
				`/api/v1/library/search?${new URLSearchParams({ q }).toString()}${all ? "&all=true" : ""}`,
			),
		listPlaylists: () =>
			request<PlaylistList>("listPlaylists", "/api/v1/playlists"),
		createPlaylist: (body: PlaylistCreate) =>
			request<Playlist>("createPlaylist", "/api/v1/playlists", {
				method: "POST",
				body: JSON.stringify(body),
			}),
		getPlaylist: (playlistId: string) =>
			request<WirePlaylistDetail>(
				"getPlaylist",
				`/api/v1/playlists/${playlistId}`,
			).then(normalizePlaylistDetail),
		addPlaylistTrack: (playlistId: string, trackId: string) =>
			request<WirePlaylistDetail>(
				"addPlaylistTrack",
				`/api/v1/playlists/${playlistId}/tracks`,
				{
					method: "POST",
					body: JSON.stringify({ trackId }),
				},
			).then(normalizePlaylistDetail),
		removePlaylistTrack: (playlistId: string, trackId: string) =>
			request<WirePlaylistDetail>(
				"removePlaylistTrack",
				`/api/v1/playlists/${playlistId}/tracks/${trackId}`,
				{
					method: "DELETE",
				},
			).then(normalizePlaylistDetail),
		listRadioStations: () =>
			request<RadioStationList>("listRadioStations", "/api/v1/radio/stations"),
		getRadioStation: (stationId: string) =>
			request<RadioStation>(
				"getRadioStation",
				`/api/v1/radio/stations/${stationId}`,
			),
		createRadioStation: (body: RadioStationCreate) =>
			request<RadioStation>("createRadioStation", "/api/v1/radio/stations", {
				method: "POST",
				body: JSON.stringify(body),
			}),
		patchRadioStation: (stationId: string, body: RadioStationPatch) =>
			request<RadioStation>(
				"patchRadioStation",
				`/api/v1/radio/stations/${stationId}`,
				{
					method: "PATCH",
					body: JSON.stringify(body),
				},
			),
		deleteRadioStation: (stationId: string) =>
			request<void>(
				"deleteRadioStation",
				`/api/v1/radio/stations/${stationId}`,
				{
					method: "DELETE",
				},
			),
		searchRadioStations: (params?: RadioSearchParams) =>
			request<RadioSearchResultList>(
				"searchRadioStations",
				`/api/v1/radio/search${buildRadioSearchQuery(params)}`,
			),
		listRadioCatalogCountries: () =>
			request<RadioCatalogOptionList>(
				"listRadioCatalogCountries",
				"/api/v1/radio/catalog/countries",
			),
		listRadioCatalogTags: () =>
			request<RadioCatalogOptionList>(
				"listRadioCatalogTags",
				"/api/v1/radio/catalog/tags",
			),
		importRadioStation: (body: RadioImportRequest) =>
			request<RadioStation>("importRadioStation", "/api/v1/radio/import", {
				method: "POST",
				body: JSON.stringify(body),
			}),
		getRadioNowPlaying: (stationId: string) =>
			request<RadioNowPlaying>(
				"getRadioNowPlaying",
				`/api/v1/radio/stations/${stationId}/now-playing`,
			),
		/** Searches the External Source; `limit` defaults to the server's 10 (at most 20). */
		searchExternalSource: (q: string, limit?: number, signal?: AbortSignal) =>
			request<ExternalSearchResponse>(
				"searchExternalSource",
				`/api/v1/external/search?${new URLSearchParams({ q }).toString()}${limit != null ? `&limit=${limit}` : ""}`,
				{ signal },
			),
		/** Resolves an External Track; play `streamPath` through getExternalStreamUrl. */
		getExternalTrackPlayback: (externalTrackId: string, signal?: AbortSignal) =>
			request<ExternalTrackPlayback>(
				"getExternalTrackPlayback",
				`/api/v1/external/tracks/${encodeURIComponent(externalTrackId)}/playback`,
				{ signal },
			),
		/** `streamPath` is an ExternalTrackPlayback's server path; keeps `.m3u8` for HLS detection. */
		getExternalStreamUrl: (streamPath: string) =>
			`${getStreamBaseUrl()}${streamPath}`,
		/** Only for an External Track whose `hasArtwork` is true. */
		getExternalTrackArtworkUrl: (externalTrackId: string) =>
			`${getMediaBaseUrl()}/api/v1/external/tracks/${encodeURIComponent(externalTrackId)}/artwork`,
		getRadioStationStreamUrl: (stationId: string) =>
			`${getStreamBaseUrl()}/api/v1/radio/stations/${stationId}/stream`,
		getRadioCatalogPreviewStreamUrl: (stationUuid: string) =>
			`${getStreamBaseUrl()}/api/v1/radio/preview/${stationUuid}/stream`,
		getTrackStreamUrl: (trackId: string) =>
			`${getStreamBaseUrl()}/api/v1/tracks/${trackId}/stream`,
		/**
		 * Checks whether a Track's stream is reachable without downloading it.
		 * Resolves with the HTTP status; rejects only when the request itself
		 * fails (offline, DNS, refused connection).
		 */
		headTrackStream: async (trackId: string): Promise<{ status: number }> => {
			const headers = new Headers();
			const token = getToken?.();
			if (token) {
				headers.set("Authorization", `Bearer ${token}`);
			}
			const response = await transport(
				`${baseUrl}/api/v1/tracks/${trackId}/stream`,
				{
					method: "HEAD",
					headers,
				},
			);
			return { status: response.status };
		},
		/** `size` fits the cover into that many pixels; omit for the original. */
		getAlbumCoverUrl: (albumId: string, size?: CoverSize) =>
			`${getMediaBaseUrl()}/api/v1/library/albums/${albumId}/cover${size ? `?size=${size}` : ""}`,
		/** Not found while the Artist has no photo; show their initial then. */
		getArtistPhotoUrl: (artistId: string) =>
			`${getMediaBaseUrl()}/api/v1/library/artists/${artistId}/photo`,
	};
}

export type ApiClient = ReturnType<typeof createApiClient>;
