import { describe, expect, expectTypeOf, it, vi } from "vite-plus/test";
import {
	ApiError,
	createApiClient,
	type QueueItem,
	type QueueItemSource,
} from "./index";

const albumSource: QueueItemSource = {
	kind: "album",
	albumId: "00000000-0000-4000-8000-000000000004",
	albumTitle: "Album",
	artistName: "Artist",
};

function queueResponse(source?: QueueItemSource) {
	return {
		revision: "2",
		items: [
			{
				id: "00000000-0000-4000-8000-000000000006",
				trackId: "00000000-0000-4000-8000-000000000001",
				position: 0,
				source,
				track: {
					id: "00000000-0000-4000-8000-000000000001",
					title: "Track",
					artistName: "Artist",
					albumId: "00000000-0000-4000-8000-000000000004",
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
			["00000000-0000-4000-8000-000000000001"],
			"1",
			albumSource,
		);
		const appended = await client.appendPlaybackQueueItem(
			"00000000-0000-4000-8000-000000000001",
			"2",
			albumSource,
		);
		expect(JSON.parse(String(transport.mock.calls[0]?.[1]?.body))).toEqual({
			trackIds: ["00000000-0000-4000-8000-000000000001"],
			revision: "1",
			source: albumSource,
		});
		expect(JSON.parse(String(transport.mock.calls[1]?.[1]?.body))).toEqual({
			trackId: "00000000-0000-4000-8000-000000000001",
			revision: "2",
			source: albumSource,
		});
		expect(replaced.items[0]?.source).toEqual(albumSource);
		expect(appended.items[0]?.source).toEqual(albumSource);
	});

	it.each<QueueItemSource | undefined>([
		undefined,
		albumSource,
		{
			kind: "playlist",
			playlistId: "00000000-0000-4000-8000-000000000012",
			name: "Playlist",
		},
		{ kind: "user" },
		{
			kind: "suggestion",
			basedOn: [
				"00000000-0000-4000-8000-000000000010",
				"00000000-0000-4000-8000-000000000011",
			],
		},
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
				await client.appendPlaybackQueueItem(
					"00000000-0000-4000-8000-000000000001",
					"0",
				);
				expect.fail("Expected revision conflict");
			} catch (error) {
				expect(error).toBeInstanceOf(ApiError);
				expect((error as ApiError).body).toMatchObject({ queue });
			}
		},
	);
});
