import type { QueueItem, Track } from "@repo/api-client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import type { PlaybackSource } from "../playback/PlaybackEngine";
import { UpNextPeek } from "./UpNextPeek";

const mocks = vi.hoisted(() => ({
	usePlayback: vi.fn(),
	playQueueIndex: vi.fn(async () => {}),
	getAlbumCoverUrl: vi.fn((albumId: string) => `/cover/${albumId}`),
}));

vi.mock("../playback/PlaybackProvider", () => ({
	usePlayback: mocks.usePlayback,
}));

const track: Track = {
	id: "repeated-track",
	title: "Repeated track",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "opus",
	genres: [],
};
const nextTrack: Track = {
	...track,
	id: "next-track",
	title: "Next track",
	albumId: "album-2",
};
const queue: QueueItem[] = [
	{ id: "first-occurrence", trackId: track.id, track, position: 0 },
	{ id: "second-occurrence", trackId: track.id, track, position: 1 },
	{ id: "next-item", trackId: nextTrack.id, track: nextTrack, position: 2 },
];

function setPlayback(
	playbackSource: PlaybackSource | null,
	currentTrack: Track | null = track,
	items = queue,
) {
	mocks.usePlayback.mockReturnValue({
		queue: items,
		currentTrack,
		playbackSource,
		playQueueIndex: mocks.playQueueIndex,
		getAlbumCoverUrl: mocks.getAlbumCoverUrl,
	});
}

describe("UpNextPeek", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		setPlayback({
			type: "track",
			track,
			playbackUrl: "/stream/repeated-track",
			queueItemId: "second-occurrence",
		});
	});
	afterEach(cleanup);

	it("uses queue item identity to find the next track after a repeated track", () => {
		render(<UpNextPeek />);
		const button = screen.getByRole("button", {
			name: "Up next: Next track by Artist. Play now",
		});
		expect(button.getAttribute("title")).toBe("Up next: Next track");
		expect(screen.getByText("Up next")).toBeTruthy();
		expect(button.querySelector("img")?.getAttribute("src")).toBe(
			"/cover/album-2",
		);
		fireEvent.click(button);
		expect(mocks.playQueueIndex).toHaveBeenCalledExactlyOnceWith(2);
	});

	it("can advance to a second occurrence of the same track", () => {
		setPlayback({
			type: "track",
			track,
			playbackUrl: "/stream/repeated-track",
			queueItemId: "first-occurrence",
		});
		render(<UpNextPeek />);
		fireEvent.click(
			screen.getByRole("button", {
				name: "Up next: Repeated track by Artist. Play now",
			}),
		);
		expect(mocks.playQueueIndex).toHaveBeenCalledExactlyOnceWith(1);
	});

	it("falls back to track identity for legacy playback without a queue item ID", () => {
		setPlayback({
			type: "track",
			track,
			playbackUrl: "/stream/repeated-track",
		});
		render(<UpNextPeek />);
		fireEvent.click(screen.getByRole("button"));
		expect(mocks.playQueueIndex).toHaveBeenCalledExactlyOnceWith(1);
	});

	it("hides at the final queue item even when the same track appeared earlier", () => {
		setPlayback(
			{
				type: "track",
				track,
				playbackUrl: "/stream/repeated-track",
				queueItemId: "second-occurrence",
			},
			track,
			queue.slice(0, 2),
		);
		render(<UpNextPeek />);
		expect(screen.queryByTestId("up-next")).toBeNull();
	});

	it.each([
		{ currentTrack: null, items: queue },
		{ currentTrack: track, items: [] },
		{ currentTrack: { ...track, id: "outside-queue" }, items: queue },
	])(
		"hides when no next queued track can be resolved: %j",
		({ currentTrack, items }) => {
			setPlayback(null, currentTrack, items);
			render(<UpNextPeek />);
			expect(screen.queryByTestId("up-next")).toBeNull();
			expect(mocks.playQueueIndex).not.toHaveBeenCalled();
		},
	);

	it.each<PlaybackSource>([
		{
			type: "radio-station",
			station: {
				id: "station-1",
				name: "Radio",
				streamUrl: "/live",
				tags: [],
				source: "manual",
				isFavorite: false,
				position: 0,
			},
			playbackUrl: "/radio/station-1",
			sourceUrl: "/live",
		},
		{
			type: "catalog-preview",
			result: {
				stationUuid: "catalog-1",
				name: "Preview",
				streamUrl: "/live",
				tags: [],
			},
			playbackUrl: "/radio/preview/catalog-1",
			sourceUrl: "/live",
		},
	])(
		"hides during $type even if a previous current track and queue remain",
		(source) => {
			setPlayback(source);
			render(<UpNextPeek />);
			expect(screen.queryByTestId("up-next")).toBeNull();
			expect(mocks.playQueueIndex).not.toHaveBeenCalled();
		},
	);
});
