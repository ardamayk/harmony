import { ApiError } from "@repo/api-client";
import { QueryClient, type QueryKey } from "@tanstack/react-query";
import { describe, expect, it } from "vite-plus/test";
import {
	parsePersistedClient,
	QUERY_CACHE_BUSTER,
	shouldPersistQuery,
} from "./query-persistence";

function query(queryKey: unknown[]) {
	const client = new QueryClient();
	client.setQueryData(queryKey, { items: [] });
	const found = client.getQueryCache().find({ queryKey, exact: true });
	if (!found) throw new Error("query not built");
	return found;
}

describe("shouldPersistQuery", () => {
	it("saves library, playlist and saved-station lists", () => {
		for (const key of [
			["library", "albums", "", ""],
			["library", "album", "album-1"],
			["library", "artist", "artist-1", "biography"],
			["playlists"],
			["playlist", "playlist-1"],
			["radio", "stations"],
			["radio", "stations", "station-1"],
		]) {
			expect([key, shouldPersistQuery(query(key))]).toEqual([key, true]);
		}
	});

	it("leaves out searches, live state and failed or pending queries", () => {
		for (const key of [
			["library", "search", "abc"],
			["radio", "stations", "station-1", "now-playing"],
			["health"],
			["preferences"],
			["desktop-mpv-status"],
		]) {
			expect([key, shouldPersistQuery(query(key))]).toEqual([key, false]);
		}
		const client = new QueryClient();
		const pending = client.getQueryCache().build(client, {
			queryKey: ["library", "albums"] as QueryKey,
		});
		expect(shouldPersistQuery(pending)).toBe(false);
	});

	it("keeps lists whose refetch failed offline, but not items that are gone", () => {
		const offline = query(["library", "albums", "", ""]);
		offline.setState({ status: "error", error: new TypeError("fetch failed") });
		expect(shouldPersistQuery(offline)).toBe(true);
		const gone = query(["library", "album", "deleted"]);
		gone.setState({
			status: "error",
			error: new ApiError(404, {
				error: "not_found",
				code: "not_found",
				message: "album not found",
			}),
		});
		expect(shouldPersistQuery(gone)).toBe(false);
	});
});

describe("parsePersistedClient", () => {
	const saved = {
		timestamp: 1_700_000_000_000,
		buster: QUERY_CACHE_BUSTER,
		clientState: { queries: [{ queryKey: ["library"] }], mutations: [] },
	};

	it("restores a saved cache as it was written", () => {
		expect(parsePersistedClient(saved)).toBe(saved);
	});

	it("drops a missing, old or corrupt save instead of throwing", () => {
		for (const value of [
			undefined,
			null,
			"saved",
			[],
			{},
			{ ...saved, timestamp: "yesterday" },
			{ ...saved, buster: undefined },
			{ ...saved, clientState: null },
			{ ...saved, clientState: { queries: {}, mutations: [] } },
			{ timestamp: saved.timestamp, clientState: saved.clientState },
		]) {
			expect(parsePersistedClient(value)).toBeUndefined();
		}
	});
});
