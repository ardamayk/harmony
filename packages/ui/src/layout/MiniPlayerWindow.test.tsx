import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	type PlaybackApi,
	PlaybackProvider,
	usePlayback,
} from "../playback/PlaybackProvider";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import { MiniPlayerWindow } from "./MiniPlayerWindow";

const track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "flac",
	genres: [],
};

const api: PlaybackApi = {
	getQueue: vi.fn(async () => ({
		items: [{ id: "item-1", trackId: track.id, position: 0, track }],
		revision: "1",
	})),
	replaceQueue: vi.fn(async () => ({ items: [], revision: "2" })),
	reorderQueue: vi.fn(async () => ({ items: [], revision: "2" })),
	appendQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
	removeQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
	clearQueue: vi.fn(async () => ({ items: [], revision: "2" })),
	getStreamUrl: (trackId) => `/stream/${trackId}`,
	getAlbumCoverUrl: (albumId) => `/cover/${albumId}`,
	getRadioStationStreamUrl: (stationId) => `/radio/${stationId}`,
	getRadioCatalogPreviewStreamUrl: (stationUuid) => `/preview/${stationUuid}`,
	getRadioNowPlaying: vi.fn(async () => ({})),
	listPlaylists: vi.fn(async () => ({ items: [], total: 0 })),
	getPlaylist: vi.fn(async (id: string) => ({
		id,
		name: id,
		isDefault: false,
		trackCount: 0,
		tracks: [],
	})),
	createPlaylist: vi.fn(async (name: string) => ({
		id: "new",
		name,
		isDefault: false,
		trackCount: 0,
	})),
	addPlaylistTrack: vi.fn(),
	removePlaylistTrack: vi.fn(),
};

function Starter() {
	const playback = usePlayback();
	return (
		<button type="button" onClick={() => void playback.playTrack(track.id)}>
			Start
		</button>
	);
}

describe("MiniPlayerWindow", () => {
	afterEach(cleanup);

	it("shows the current track, drives playback, and exposes expand and close", async () => {
		const engine = new InMemoryPlaybackEngine();
		const onExpand = vi.fn();
		const onClose = vi.fn();
		render(
			<PlaybackProvider api={api} engine={engine}>
				<Starter />
				<MiniPlayerWindow onExpand={onExpand} onClose={onClose} />
			</PlaybackProvider>,
		);
		expect(screen.getByText("Nothing playing")).toBeTruthy();
		// Let the provider load the Queue before starting playback from it.
		await act(async () => {});

		await act(async () => {
			screen.getByRole("button", { name: "Start" }).click();
		});
		const root = screen.getByTestId("mini-player-window");
		expect(root.getAttribute("data-window-drag-region")).not.toBeNull();
		expect(root.textContent).toContain("Track 1");

		fireEvent.click(screen.getByRole("button", { name: "Pause" }));
		expect(engine.getState().status).toBe("paused");

		fireEvent.click(screen.getByRole("button", { name: "Open main window" }));
		fireEvent.click(screen.getByRole("button", { name: "Close mini player" }));
		expect(onExpand).toHaveBeenCalledOnce();
		expect(onClose).toHaveBeenCalledOnce();
	});
});
