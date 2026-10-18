import type { Album, Track } from "@repo/api-client";
import { describe, expect, it, vi } from "vite-plus/test";
import { queueTracksByAlbum } from "./queue-tracks-by-album";

function makeTrack(id: string, albumId: string): Track {
	return {
		id,
		albumId,
		title: id,
		artistName: "Track performer",
		albumTitle: "Stale title",
		durationMs: 1000,
		format: "flac",
		artists: [],
		discNo: 1,
		genres: [],
	};
}

function loadAlbum(albumId: string): Promise<Album> {
	return Promise.resolve({
		id: albumId,
		title: `Album ${albumId}`,
		artistId: "artist-1",
		artistName: "Legacy credit",
		albumArtists: [{ id: "artist-1", name: "Album artist" }],
		genreItems: [],
		releaseIdentifiers: [],
	});
}

describe("queueTracksByAlbum", () => {
	it("appends consecutive album runs sequentially, preserving A B A order and duplicate tracks", async () => {
		const tracks = [
			makeTrack("t1", "a"),
			makeTrack("t2", "a"),
			makeTrack("t3", "b"),
			makeTrack("t1", "a"),
		];
		let completeFirst!: () => void;
		const firstAppend = new Promise<void>((resolve) => {
			completeFirst = resolve;
		});
		const queueTracks = vi
			.fn()
			.mockImplementationOnce(() => firstAppend)
			.mockResolvedValue(undefined);
		const getAlbum = vi.fn(loadAlbum);
		const pending = queueTracksByAlbum(tracks, queueTracks, getAlbum);
		await vi.waitFor(() => expect(queueTracks).toHaveBeenCalledTimes(1));
		expect(queueTracks).toHaveBeenNthCalledWith(1, ["t1", "t2"], {
			kind: "album",
			albumId: "a",
			albumTitle: "Album a",
			artistName: "Album artist",
		});
		expect(getAlbum).toHaveBeenCalledExactlyOnceWith("a");
		completeFirst();
		await pending;
		expect(queueTracks).toHaveBeenNthCalledWith(2, ["t3"], {
			kind: "album",
			albumId: "b",
			albumTitle: "Album b",
			artistName: "Album artist",
		});
		expect(queueTracks).toHaveBeenNthCalledWith(3, ["t1"], {
			kind: "album",
			albumId: "a",
			albumTitle: "Album a",
			artistName: "Album artist",
		});
		expect(queueTracks).toHaveBeenCalledTimes(3);
		expect(getAlbum.mock.calls).toEqual([["a"], ["b"]]);
		expect(tracks.map((track) => track.id)).toEqual(["t1", "t2", "t3", "t1"]);
	});

	it("does nothing for an empty selection", async () => {
		const queueTracks = vi.fn();
		const getAlbum = vi.fn(loadAlbum);
		await queueTracksByAlbum([], queueTracks, getAlbum);
		expect(queueTracks).not.toHaveBeenCalled();
		expect(getAlbum).not.toHaveBeenCalled();
	});

	it.each(["lookup", "append"])(
		"stops and propagates a failed %s instead of skipping tracks",
		async (operation) => {
			const error = new Error("Network unavailable");
			const queueTracks = vi.fn().mockResolvedValue(undefined);
			const getAlbum = vi.fn(loadAlbum);
			if (operation === "lookup") getAlbum.mockRejectedValueOnce(error);
			else queueTracks.mockRejectedValueOnce(error);
			await expect(
				queueTracksByAlbum(
					[makeTrack("t1", "a"), makeTrack("t2", "b")],
					queueTracks,
					getAlbum,
				),
			).rejects.toBe(error);
			expect(getAlbum).toHaveBeenCalledExactlyOnceWith("a");
			expect(queueTracks).toHaveBeenCalledTimes(operation === "lookup" ? 0 : 1);
		},
	);
});
