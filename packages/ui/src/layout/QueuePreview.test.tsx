import type { QueueItem, Track } from "@repo/api-client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { QueuePreview } from "./QueuePreview";

const mocks = vi.hoisted(() => ({
	usePlayback: vi.fn(),
	playQueueIndex: vi.fn(async () => {}),
}));

vi.mock("../playback/PlaybackProvider", () => ({
	usePlayback: mocks.usePlayback,
}));

const queue: QueueItem[] = Array.from({ length: 12 }, (_, position) => {
	const track: Track = {
		id: `track-${position}`,
		title: `Track ${position}`,
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 180000,
		format: "flac",
		genres: [],
	};
	return { id: `item-${position}`, trackId: track.id, track, position };
});

function renderAt(current: number) {
	const item = queue[current];
	mocks.usePlayback.mockReturnValue({
		queue,
		currentTrack: item.track,
		playbackSource: {
			type: "track",
			track: item.track,
			playbackUrl: "/stream",
			queueItemId: item.id,
		},
		playQueueIndex: mocks.playQueueIndex,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	});
	render(<QueuePreview />);
	return screen.getAllByRole("button");
}

describe("QueuePreview", () => {
	afterEach(cleanup);

	it("shows four Tracks either side of the current one, fading with distance", () => {
		const rows = renderAt(6);
		expect(rows.map((row) => row.textContent?.split("Artist")[0])).toEqual(
			[2, 3, 4, 5, 6, 7, 8, 9, 10].map((position) => `Track ${position}`),
		);
		expect(rows[4].getAttribute("aria-current")).toBe("true");
		expect(rows[0].className).toContain("opacity-25");
		expect(rows[3].className).toContain("opacity-80");
	});

	it("starts at the top of the Queue and plays a clicked row", () => {
		const rows = renderAt(1);
		expect(rows).toHaveLength(6);
		fireEvent.click(rows[5]);
		expect(mocks.playQueueIndex).toHaveBeenCalledWith(5);
	});
});
