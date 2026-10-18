import type {
	PersistedClient,
	Persister,
} from "@tanstack/react-query-persist-client";
import { ApiError } from "@repo/api-client";
import type { Query } from "@tanstack/react-query";

/**
 * Library, playlist and saved-station lists are kept on disk (IndexedDB) so
 * the Desktop Client can still browse them while its Music Server cannot be
 * reached. Search results, playback and server status are left out: they
 * are either endless variations or only true right now.
 */

/**
 * Saved lists older than this are dropped rather than shown. Also the
 * saved scopes' gcTime, which must fit a setTimeout (under 24.8 days) or
 * the query is dropped at once.
 */
export const QUERY_CACHE_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
/** Bump when a persisted response changes shape, to drop old saves. */
export const QUERY_CACHE_BUSTER = "1";

const DATABASE = "earthly-query-cache";
const STORE = "clients";
const WRITE_DELAY_MS = 1000;

/**
 * A 404 means the item is gone: pages that otherwise keep showing cached
 * data after a failed request say "not found" instead.
 */
export function isGone(error: unknown): boolean {
	return error instanceof ApiError && error.status === 404;
}

export function shouldPersistQuery(query: Query): boolean {
	// A refetch that failed offline still holds its data and stays saved;
	// a 404 does not.
	if (query.state.data === undefined || isGone(query.state.error)) {
		return false;
	}
	const [scope, kind] = query.queryKey;
	switch (scope) {
		case "library":
			return kind !== "search";
		case "playlists":
		case "playlist":
			return true;
		case "radio":
			// The saved-station list and details, not what is playing now.
			return !query.queryKey.includes("now-playing");
		default:
			return false;
	}
}

function openDatabase(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(DATABASE, 1);
		request.onupgradeneeded = () => request.result.createObjectStore(STORE);
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

async function run<T>(
	mode: IDBTransactionMode,
	operation: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
	const database = await openDatabase();
	try {
		return await new Promise<T>((resolve, reject) => {
			const request = operation(
				database.transaction(STORE, mode).objectStore(STORE),
			);
			request.onsuccess = () => resolve(request.result);
			request.onerror = () => reject(request.error);
		});
	} finally {
		database.close();
	}
}

/**
 * One saved cache per Music Server: `serverKey` resolves to the server's
 * origin, so switching servers never shows another server's lists. Writes
 * are coalesced, since the cache changes on every fetch.
 */
export function createQueryPersister(
	serverKey: () => Promise<string>,
): Persister {
	let pending: PersistedClient | null = null;
	let timer: ReturnType<typeof setTimeout> | null = null;
	const write = async () => {
		timer = null;
		const client = pending;
		pending = null;
		if (!client) return;
		const key = await serverKey();
		await run("readwrite", (store) => store.put(client, key)).catch(
			(error: unknown) => console.warn("Could not save lists offline", error),
		);
	};
	return {
		persistClient: (client) => {
			pending = client;
			timer ??= setTimeout(() => void write(), WRITE_DELAY_MS);
		},
		restoreClient: async () => {
			const key = await serverKey();
			return run<PersistedClient | undefined>("readonly", (store) =>
				store.get(key),
			).catch(() => undefined);
		},
		removeClient: async () => {
			const key = await serverKey();
			await run("readwrite", (store) => store.delete(key)).catch(
				() => undefined,
			);
		},
	};
}
