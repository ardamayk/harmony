import { describe, expect, expectTypeOf, it, vi } from "vite-plus/test";
import {
	ApiError,
	createApiClient,
	type QueueItem,
	type QueueItemSource,
} from "./index";

const albumSource: QueueItemSource = {
	kind: "album",
	albumId: "album-1",
	albumTitle: "Album",
	artistName: "Artist",
};

function queueResponse(source?: QueueItemSource) {
	return {
		revision: "2",
		items: [
			{
				id: "item-1",
				trackId: "track-1",
				position: 0,
				source,
				track: {
					id: "track-1",
					title: "Track",
					artistName: "Artist",
					albumId: "album-1",
					durationMs: 1000,
					format: "flac",
				},
			},
		],
	};
}

describe("queue sources", () => {
	it("exports the discriminated union while keeping item source optional", () => {
		expectTypeOf<QueueItem["source"]>().toEqualTypeOf<
			QueueItemSource | undefined
		>();
		expectTypeOf<QueueItemSource>().toEqualTypeOf<
			| {
					kind: "album";
					albumId: string;
					albumTitle: string;
					artistName: string;
			  }
			| { kind: "playlist"; playlistId: string; name: string }
			| { kind: "user" }
			| { kind: "suggestion"; basedOn: string[] }
		>();
	});

	it("sends source with replace and append while preserving revisions", async () => {
		const transport = vi.fn<typeof fetch>(async () =>
			Response.json(queueResponse(albumSource)),
		);
		const client = createApiClient({ baseUrl: "", transport });
		const replaced = await client.replacePlaybackQueue(
			["track-1"],
			"1",
			albumSource,
		);
		const appended = await client.appendPlaybackQueueItem(
			"track-1",
			"2",
			albumSource,
		);
		expect(JSON.parse(String(transport.mock.calls[0]?.[1]?.body))).toEqual({
			trackIds: ["track-1"],
			revision: "1",
			source: albumSource,
		});
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			trackId: "track-1",
			revision: "2",
			source: albumSource,
		});
		expect(replaced.items[0]?.source).toEqual(albumSource);
		expect(appended.items[0]?.source).toEqual(albumSource);
	});

	it.each<QueueItemSource | undefined>([
		undefined,
		albumSource,
		{ kind: "playlist", playlistId: "playlist-1", name: "Playlist" },
		{ kind: "user" },
		{ kind: "suggestion", basedOn: ["seed-1", "seed-2"] },
		{ kind: "suggestion", basedOn: [] },
	])(
		"normalizes reads and conflict snapshots with source %j",
		async (source) => {
			const transport = vi
				.fn<typeof fetch>()
				.mockResolvedValueOnce(Response.json(queueResponse(source)))
				.mockResolvedValueOnce(
					Response.json(
						{
							error: "conflict",
							code: "queue_revision_conflict",
							message: "Queue changed",
							queue: queueResponse(source),
						},
						{ status: 409 },
					),
				);
			const client = createApiClient({ baseUrl: "", transport });
			const queue = await client.getPlaybackQueue();
			expect(queue.items[0]?.source).toEqual(source ?? { kind: "user" });
			try {
				await client.appendPlaybackQueueItem("track-1", "0");
				expect.fail("Expected revision conflict");
			} catch (error) {
				expect(error).toBeInstanceOf(ApiError);
				expect((error as ApiError).body).toMatchObject({ queue });
			}
		},
	);
});
