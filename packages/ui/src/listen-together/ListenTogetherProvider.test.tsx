import {
	ApiError,
	type ListenTogetherCommand,
	type ListenTogetherRoom,
	type RoomEvent,
} from "@repo/api-client";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { useLayoutEffect } from "react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	type Mock,
	vi,
} from "vite-plus/test";
import {
	type PlaybackApi,
	PlaybackProvider,
	usePlayback,
} from "../playback/PlaybackProvider";
import { InMemoryPlaybackEngine } from "../playback/testing/InMemoryPlaybackEngine";
import {
	type ListenTogetherContextValue,
	useListenTogether,
} from "./ListenTogetherContext";
import {
	type ListenTogetherApi,
	ListenTogetherProvider,
} from "./ListenTogetherProvider";
import { RoomSyncDriver } from "./RoomSyncDriver";

const toastError = vi.fn();
vi.mock("sonner", () => ({
	toast: {
		error: (...args: unknown[]) => toastError(...args),
		success: vi.fn(),
	},
}));

function createTrack(id: string) {
	return {
		id,
		title: `Title ${id}`,
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120_000,
		format: "flac",
		genres: [],
	};
}

const baseRoom: ListenTogetherRoom = {
	revision: 1,
	trackIds: ["track-1", "track-2"],
	currentIndex: 0,
	playing: false,
	positionMs: 0,
	positionAtMs: 0,
	lyricsOpen: false,
	guestCount: 0,
	maxGuests: 8,
	expiresAt: "2026-10-31T12:00:00Z",
};

function notFound() {
	return new ApiError(404, { error: "x", code: "not_found", message: "gone" });
}

function createApi(
	options: {
		role?: "owner" | "guest" | "none";
		room?: ListenTogetherRoom | null;
		capabilities?: string[];
	} = {},
) {
	const { role = "owner", capabilities = ["listen-together.v1"] } = options;
	const room = options.room === undefined ? baseRoom : options.room;
	const listeners: ((event: RoomEvent) => void)[] = [];
	const unsubscribe = vi.fn();
	const api = {
		getHealth: vi.fn(async () => ({
			status: "ok",
			version: "1",
			capabilities,
			dependencies: {},
		})),
		getListenTogetherSession: vi.fn(async () => ({ role })),
		joinListenTogetherRoom: vi.fn(async () => baseRoom),
		leaveListenTogetherRoom: vi.fn(async () => undefined),
		getListenTogetherRoom: vi.fn(async () => {
			if (!room) throw notFound();
			return room;
		}),
		openListenTogetherRoom: vi.fn(async () => ({
			...baseRoom,
			inviteUrl: "https://example.com/join#token",
		})),
		closeListenTogetherRoom: vi.fn(async () => undefined),
		sendListenTogetherCommand: vi.fn(async () => ({
			...baseRoom,
			revision: 10,
		})),
		getListenTogetherClock: vi.fn(async () => ({ serverTimeMs: Date.now() })),
		subscribeListenTogetherRoomEvents: vi.fn(
			(onEvent: (event: RoomEvent) => void) => {
				listeners.push(onEvent);
				return unsubscribe;
			},
		),
		getTrack: vi.fn(async (trackId: string) => createTrack(trackId)),
		getTrackStreamUrl: (trackId: string) => `/stream/${trackId}`,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	};
	return {
		api: api as unknown as ListenTogetherApi,
		calls: api,
		unsubscribe,
		emit: (event: RoomEvent) => {
			for (const listener of listeners) listener(event);
		},
	};
}

function createPlaybackApi(): PlaybackApi {
	return {
		getQueue: vi.fn(async () => ({ items: [], revision: "1" })),
		replaceQueue: vi.fn(async () => ({ items: [], revision: "2" })),
		reorderQueue: vi.fn(async () => ({ items: [], revision: "2" })),
		appendQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
		removeQueueItem: vi.fn(async () => ({ items: [], revision: "2" })),
		clearQueue: vi.fn(async () => ({ items: [], revision: "2" })),
		subscribeQueueEvents: vi.fn(() => vi.fn()),
		getStreamUrl: (trackId: string) => `/stream/${trackId}`,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		getRadioStationStreamUrl: (id: string) => `/radio/${id}`,
		getRadioCatalogPreviewStreamUrl: (id: string) => `/radio/preview/${id}`,
		getRadioNowPlaying: vi.fn(async () => ({})),
		getExternalTrackPlayback: vi.fn(),
		getExternalStreamUrl: (path: string) => path,
		getExternalTrackArtworkUrl: (id: string) => `/artwork/${id}`,
		listPlaylists: vi.fn(),
		getPlaylist: vi.fn(),
		createPlaylist: vi.fn(),
		addPlaylistTrack: vi.fn(),
		removePlaylistTrack: vi.fn(),
	} as unknown as PlaybackApi;
}

let context: ListenTogetherContextValue;
let playback: ReturnType<typeof usePlayback>;

function Probe() {
	const listenTogether = useListenTogether();
	const currentPlayback = usePlayback();
	useLayoutEffect(() => {
		context = listenTogether;
		playback = currentPlayback;
	});
	return (
		<div>
			<span data-testid="role">{listenTogether.role}</span>
			<span data-testid="revision">{listenTogether.room?.revision ?? ""}</span>
			<span data-testid="ending">{listenTogether.ending ?? ""}</span>
			<span data-testid="queue">
				{currentPlayback.queue.map((item) => item.track.id).join(",")}
			</span>
		</div>
	);
}

function renderProvider(
	fixture: ReturnType<typeof createApi>,
	engine = new InMemoryPlaybackEngine(),
) {
	const playbackApi = createPlaybackApi();
	render(
		<ListenTogetherProvider api={fixture.api} engine={engine}>
			<PlaybackProvider api={playbackApi} engine={engine}>
				<Probe />
			</PlaybackProvider>
		</ListenTogetherProvider>,
	);
	return { engine, playbackApi };
}

async function roleIs(role: string) {
	await waitFor(() =>
		expect(screen.getByTestId("role").textContent).toBe(role),
	);
}

describe("ListenTogetherProvider", () => {
	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("starts loading, then knows the role and the capability", async () => {
		renderProvider(createApi({ room: null }));

		expect(screen.getByTestId("role").textContent).toBe("loading");
		await roleIs("owner");
		expect(context.isAvailable).toBe(true);
		expect(context.room).toBeNull();
	});

	it("is unavailable without the capability", async () => {
		const fixture = createApi({ capabilities: [] });
		renderProvider(fixture);

		await roleIs("owner");

		expect(context.isAvailable).toBe(false);
		expect(fixture.calls.getListenTogetherRoom).not.toHaveBeenCalled();
	});

	it("notices a Music Server restarted with Listen Together on focus", async () => {
		const fixture = createApi({ capabilities: [] });
		renderProvider(fixture);
		await roleIs("owner");
		expect(context.isAvailable).toBe(false);

		fixture.calls.getHealth.mockResolvedValue({
			status: "ok",
			version: "1",
			capabilities: ["listen-together.v1"],
			dependencies: {},
		});
		// Repeated, since the listener arrives with the effect after the role.
		await waitFor(() => {
			act(() => {
				window.dispatchEvent(new Event("focus"));
			});
			expect(context.isAvailable).toBe(true);
		});
		expect(context.room?.revision).toBe(baseRoom.revision);
	});

	it("keeps the Owner in when an older server answers with a page", async () => {
		const fixture = createApi({ capabilities: [] });
		fixture.calls.getListenTogetherSession.mockRejectedValue(
			new SyntaxError("Unexpected token '<'"),
		);
		renderProvider(fixture);

		await roleIs("owner");
		expect(context.role).toBe("owner");
	});

	it("shows a visitor nothing when a Guest Listener's session fails", async () => {
		const fixture = createApi();
		fixture.calls.getListenTogetherSession.mockRejectedValue(
			new Error("network down"),
		);
		renderProvider(fixture);

		await roleIs("none");
		expect(context.role).toBe("none");
	});

	it("shows nothing when neither the session nor the health answers", async () => {
		const fixture = createApi({ capabilities: [] });
		fixture.calls.getListenTogetherSession.mockRejectedValue(
			new Error("network down"),
		);
		fixture.calls.getHealth.mockRejectedValue(new Error("network down"));
		renderProvider(fixture);

		await roleIs("none");
		expect(context.role).toBe("none");
	});

	it("treats a server without the session endpoint as the Owner's", async () => {
		const fixture = createApi();
		fixture.calls.getListenTogetherSession.mockRejectedValue(notFound());
		renderProvider(fixture);

		await roleIs("owner");
		expect(context.role).toBe("owner");
	});

	it("picks up an open Room on boot", async () => {
		const fixture = createApi({ room: { ...baseRoom, revision: 5 } });
		renderProvider(fixture);

		await waitFor(() =>
			expect(screen.getByTestId("revision").textContent).toBe("5"),
		);
		await waitFor(() =>
			expect(
				fixture.calls.subscribeListenTogetherRoomEvents,
			).toHaveBeenCalled(),
		);
	});

	it("reads no personal Queue for a Guest", async () => {
		const fixture = createApi({ role: "guest" });
		const { playbackApi } = renderProvider(fixture);

		await roleIs("guest");
		await waitFor(() =>
			expect(screen.getByTestId("queue").textContent).toBe("track-1,track-2"),
		);

		expect(playbackApi.getQueue).not.toHaveBeenCalled();
		expect(playbackApi.subscribeQueueEvents).not.toHaveBeenCalled();
	});

	it("reads no personal Queue for a visitor without a Room", async () => {
		const fixture = createApi({ role: "none", room: null });
		const { playbackApi } = renderProvider(fixture);

		await roleIs("none");

		expect(playbackApi.getQueue).not.toHaveBeenCalled();
	});

	it("keeps the Owner's Queue and shows the Room Queue over it", async () => {
		const fixture = createApi();
		const { playbackApi } = renderProvider(fixture);

		await waitFor(() =>
			expect(screen.getByTestId("queue").textContent).toBe("track-1,track-2"),
		);

		expect(playbackApi.getQueue).toHaveBeenCalled();
		expect(playbackApi.replaceQueue).not.toHaveBeenCalled();
	});

	it("gives a Room Queue with one Track twice a place for each", async () => {
		const fixture = createApi({
			room: { ...baseRoom, trackIds: ["track-1", "track-1"] },
		});
		renderProvider(fixture);

		await waitFor(() => expect(playback.queue).toHaveLength(2));

		expect(new Set(playback.queue.map((item) => item.id)).size).toBe(2);
	});

	it("ignores a snapshot older than the newest seen", async () => {
		const fixture = createApi({ room: { ...baseRoom, revision: 5 } });
		renderProvider(fixture);
		await waitFor(() =>
			expect(screen.getByTestId("revision").textContent).toBe("5"),
		);
		await waitFor(() =>
			expect(
				fixture.calls.subscribeListenTogetherRoomEvents,
			).toHaveBeenCalled(),
		);

		act(() =>
			fixture.emit({
				type: "room-state",
				room: { ...baseRoom, revision: 4 },
			}),
		);
		expect(screen.getByTestId("revision").textContent).toBe("5");

		act(() =>
			fixture.emit({
				type: "room-state",
				room: { ...baseRoom, revision: 6 },
			}),
		);
		expect(screen.getByTestId("revision").textContent).toBe("6");
	});

	it("keeps the Invite Link across snapshots that omit it", async () => {
		const fixture = createApi({ room: null });
		renderProvider(fixture);
		await roleIs("owner");
		await act(() => context.openRoom());

		act(() =>
			fixture.emit({
				type: "room-state",
				room: { ...baseRoom, revision: 2 },
			}),
		);

		expect(context.room?.inviteUrl).toBe("https://example.com/join#token");
	});

	it("adopts the Room when another client opened it first", async () => {
		const fixture = createApi({ room: null });
		fixture.calls.openListenTogetherRoom.mockRejectedValue(
			new ApiError(409, { error: "x", code: "conflict", message: "open" }),
		);
		renderProvider(fixture);
		await roleIs("owner");
		fixture.calls.getListenTogetherRoom.mockResolvedValue(baseRoom);

		await act(() => context.openRoom());

		expect(context.room?.revision).toBe(1);
	});

	it("joins as a Guest from a click, unlocking audio first", async () => {
		const fixture = createApi({ role: "none", room: null });
		const engine = new InMemoryPlaybackEngine();
		const unlockAudio = vi.fn();
		Object.assign(engine, { unlockAudio });
		renderProvider(fixture, engine);
		await roleIs("none");

		await act(() => context.joinRoom("token"));

		expect(unlockAudio).toHaveBeenCalledOnce();
		expect(fixture.calls.joinListenTogetherRoom).toHaveBeenCalledWith("token");
		await roleIs("guest");
		expect(context.room).not.toBeNull();
		expect(context.ending).toBeNull();
	});

	it("rejects a failed join with the API error", async () => {
		const fixture = createApi({ role: "none", room: null });
		const failure = new ApiError(409, {
			error: "x",
			code: "room_full",
			message: "Full",
		});
		fixture.calls.joinListenTogetherRoom.mockRejectedValue(failure);
		renderProvider(fixture);
		await roleIs("none");

		await expect(context.joinRoom("token")).rejects.toBe(failure);
	});

	it("ends with closed on room-closed, stopping a Guest", async () => {
		const fixture = createApi({ role: "guest" });
		const { engine } = renderProvider(fixture);
		await waitFor(() => expect(engine.getState().source).not.toBeNull());

		act(() => fixture.emit({ type: "room-closed" }));

		expect(screen.getByTestId("ending").textContent).toBe("closed");
		expect(context.room).toBeNull();
		expect(engine.getState().source).toBeNull();
		await roleIs("none");
		expect(fixture.unsubscribe).toHaveBeenCalled();
	});

	it("ends with closed when a command finds the Room gone", async () => {
		const fixture = createApi({ role: "guest" });
		fixture.calls.sendListenTogetherCommand.mockRejectedValue(
			new ApiError(401, { error: "x", code: "unauthorized", message: "no" }),
		);
		renderProvider(fixture);
		await waitFor(() => expect(context.room).not.toBeNull());

		await act(async () => playback.togglePlay());

		await waitFor(() =>
			expect(screen.getByTestId("ending").textContent).toBe("closed"),
		);
	});

	it("leaves the Owner's Player paused when the Room closes", async () => {
		const fixture = createApi({ room: { ...baseRoom, playing: true } });
		const { engine } = renderProvider(fixture);
		await waitFor(() => expect(engine.getState().status).toBe("playing"));

		await act(() => context.closeRoom());

		expect(context.ending).toBe("closed");
		expect(engine.getState().status).toBe("paused");
		expect(engine.getState().source).not.toBeNull();
	});

	it("ends with left after leaving", async () => {
		const fixture = createApi({ role: "guest" });
		const { engine } = renderProvider(fixture);
		await waitFor(() => expect(engine.getState().source).not.toBeNull());

		await act(() => context.leaveRoom());

		expect(context.ending).toBe("left");
		expect(engine.getState().source).toBeNull();
	});

	it("still leaves when the session has already gone", async () => {
		const fixture = createApi({ role: "guest" });
		fixture.calls.leaveListenTogetherRoom.mockRejectedValue(
			new ApiError(401, { error: "x", code: "unauthorized", message: "no" }),
		);
		renderProvider(fixture);
		await waitFor(() => expect(context.room).not.toBeNull());

		await act(() => context.leaveRoom());

		expect(context.ending).toBe("left");
	});

	it("shows a rate limit as a toast and stays in the Room", async () => {
		const fixture = createApi();
		fixture.calls.sendListenTogetherCommand.mockRejectedValue(
			new ApiError(429, { error: "x", code: "rate_limited", message: "slow" }),
		);
		renderProvider(fixture);
		await waitFor(() => expect(context.room).not.toBeNull());

		await act(async () => playback.togglePlay());

		await waitFor(() => expect(toastError).toHaveBeenCalled());
		expect(context.room).not.toBeNull();
	});
});

describe("playback during a Room", () => {
	// These pin what the Provider sends; following the Room is RoomSyncDriver's.
	beforeEach(() => {
		vi.spyOn(RoomSyncDriver.prototype, "start").mockImplementation(() => {});
	});

	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
		vi.clearAllMocks();
	});

	async function inRoom(room = baseRoom) {
		const fixture = createApi({ room });
		const engine = new InMemoryPlaybackEngine();
		const spies = {
			play: vi.spyOn(engine, "play"),
			pause: vi.spyOn(engine, "pause"),
			seek: vi.spyOn(engine, "seek"),
		};
		const { playbackApi } = renderProvider(fixture, engine);
		await waitFor(() => expect(playback.queue.length).toBeGreaterThan(0));
		const commands = fixture.calls.sendListenTogetherCommand as unknown as Mock<
			(command: ListenTogetherCommand) => Promise<ListenTogetherRoom>
		>;
		return { fixture, engine, playbackApi, spies, commands };
	}

	it("sends the transport as commands and leaves the engine alone", async () => {
		const { spies, commands } = await inRoom();

		await act(async () => {
			playback.togglePlay();
			playback.seek(12.5);
			playback.navigateNext();
			playback.navigatePrevious();
		});

		expect(commands.mock.calls.map(([command]) => command)).toEqual([
			{ type: "resume" },
			{ type: "seek", positionMs: 12_500 },
			{ type: "skip", direction: "next" },
			{ type: "skip", direction: "previous" },
		]);
		expect(spies.play).not.toHaveBeenCalled();
		expect(spies.pause).not.toHaveBeenCalled();
		expect(spies.seek).not.toHaveBeenCalled();
	});

	it("plays a library Track with its context as the Room Queue", async () => {
		const { commands, playbackApi } = await inRoom();

		await act(() => playback.playTrack("track-9", ["track-8", "track-9"]));

		expect(commands).toHaveBeenCalledWith({
			type: "playTracks",
			trackIds: ["track-8", "track-9"],
			index: 1,
		});
		expect(playbackApi.replaceQueue).not.toHaveBeenCalled();
	});

	it("plays a place in the Room Queue", async () => {
		const { commands } = await inRoom();

		await act(() => playback.playQueueIndex(1));

		expect(commands).toHaveBeenCalledWith({
			type: "playTracks",
			trackIds: ["track-1", "track-2"],
			index: 1,
		});
	});

	it("refuses other Playback Sources with a toast", async () => {
		const { spies, playbackApi } = await inRoom();

		await act(async () => {
			await playback.playRadioStation({ id: "s" } as never);
			await playback.playRadioCatalogPreview({ stationUuid: "c" } as never);
			await playback.playExternalTrack({ id: "e" } as never);
		});

		expect(toastError).toHaveBeenCalledTimes(3);
		expect(spies.play).not.toHaveBeenCalled();
		expect(playbackApi.getExternalTrackPlayback).not.toHaveBeenCalled();
	});

	it("leaves the Owner's Queue unchanged", async () => {
		const { playbackApi } = await inRoom();

		await act(async () => {
			await playback.addToQueue("track-9");
			await playback.playNext("track-9");
			await playback.removeFromQueue("room:0:track-1");
			await playback.reorderQueue(["room:0:track-1"]);
			await playback.clearQueue();
		});

		expect(playbackApi.appendQueueItem).not.toHaveBeenCalled();
		expect(playbackApi.removeQueueItem).not.toHaveBeenCalled();
		expect(playbackApi.reorderQueue).not.toHaveBeenCalled();
		expect(playbackApi.clearQueue).not.toHaveBeenCalled();
	});

	it("keeps the speed, shuffle and repeat out of the Room", async () => {
		const { engine } = await inRoom();

		act(() => {
			playback.setPlaybackRate(2);
			playback.toggleShuffle();
			playback.cycleRepeatMode();
		});

		expect(engine.getState().playbackRate ?? 1).toBe(1);
		expect(engine.getState().shuffleEnabled).toBe(false);
		expect(engine.getState().repeatMode).toBe("off");
	});

	it("does not advance through the personal Queue at the end of a Track", async () => {
		const { engine, playbackApi } = await inRoom();
		await act(async () => {
			await engine.play({
				type: "track",
				track: createTrack("track-1"),
				playbackUrl: "/stream/track-1",
				queueItemId: "room:0:track-1",
			});
		});
		const stop = vi.spyOn(engine, "stop");

		act(() => engine.finish());

		expect(stop).not.toHaveBeenCalled();
		expect(playbackApi.getQueue).toHaveBeenCalledTimes(1);
	});

	it("turns Media Session navigation into a skip", async () => {
		const { engine, commands } = await inRoom();

		await act(async () => engine.next());

		expect(commands).toHaveBeenCalledWith({ type: "skip", direction: "next" });
	});

	it("restarts the Track through the Room late in it", async () => {
		const { engine, commands } = await inRoom();
		act(() => {
			void engine.play({
				type: "track",
				track: createTrack("track-1"),
				playbackUrl: "/stream/track-1",
			});
			engine.seek(45);
		});

		await act(async () => playback.navigatePrevious());

		expect(commands).toHaveBeenCalledWith({ type: "seek", positionMs: 0 });
	});

	it("does not count down to a skip when this Player fails", async () => {
		const { engine } = await inRoom();
		await act(async () => {
			await engine.play({
				type: "track",
				track: createTrack("track-1"),
				playbackUrl: "/stream/track-1",
			});
		});

		act(() => engine.fail({ code: "playback-failed", message: "Failed" }));

		expect(playback.errorRecovery?.countdownSeconds).toBeNull();
	});
});
