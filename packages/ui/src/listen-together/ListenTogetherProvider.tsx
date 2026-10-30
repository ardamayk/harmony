import {
	type ApiClient,
	ApiError,
	hasServerCapability,
	isListenTogetherRoomGone,
	LISTEN_TOGETHER_CAPABILITY,
	type ListenTogetherCommand,
	type ListenTogetherRoom,
	type QueueItem,
	type Track,
} from "@repo/api-client";
import {
	type ReactNode,
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { toast } from "sonner";
import type {
	PlaybackEngine,
	PlaybackSource,
} from "../playback/PlaybackEngine";
import {
	ListenTogetherContext,
	type ListenTogetherContextValue,
	type ListenTogetherEnding,
	type ListenTogetherRole,
} from "./ListenTogetherContext";
import { type RoomPlayback, RoomPlaybackContext } from "./RoomPlaybackContext";
import { RoomSyncDriver } from "./RoomSyncDriver";
import { isStaleSnapshot, roomQueueItemId } from "./roomSync";

export type ListenTogetherApi = Pick<
	ApiClient,
	| "getHealth"
	| "getListenTogetherSession"
	| "joinListenTogetherRoom"
	| "leaveListenTogetherRoom"
	| "getListenTogetherRoom"
	| "openListenTogetherRoom"
	| "closeListenTogetherRoom"
	| "sendListenTogetherCommand"
	| "getListenTogetherClock"
	| "subscribeListenTogetherRoomEvents"
	| "getTrack"
	| "getTrackStreamUrl"
	| "getAlbumCoverUrl"
>;

/** Room Queue Tracks read at once; a Room Queue can hold 500. */
const TRACK_LOAD_CONCURRENCY = 6;

/** Null when the Music Server gave no usable answer. */
async function fetchRole(
	api: ListenTogetherApi,
): Promise<ListenTogetherRole | null> {
	try {
		return (await api.getListenTogetherSession()).role;
	} catch (error) {
		// A Music Server from before Listen Together has no Guests to tell apart.
		if (error instanceof ApiError && error.status === 404) return "owner";
		console.warn("Failed to read the Listen Together session", { error });
		return null;
	}
}

async function fetchRoom(
	api: ListenTogetherApi,
): Promise<ListenTogetherRoom | null> {
	try {
		return await api.getListenTogetherRoom();
	} catch (error) {
		if (!isListenTogetherRoomGone(error)) {
			console.warn("Failed to read the Room", { error });
		}
		return null;
	}
}

function createTrackCache(getTrack: (trackId: string) => Promise<Track>) {
	const tracks = new Map<string, Promise<Track>>();
	return (trackId: string) => {
		const cached = tracks.get(trackId);
		if (cached) return cached;
		const loading = getTrack(trackId);
		tracks.set(trackId, loading);
		loading.catch(() => tracks.delete(trackId));
		return loading;
	};
}

/** Resolves the Room Queue's Tracks with the library call every client has. */
function useRoomQueue(
	trackIds: string[] | null,
	loadTrack: (trackId: string) => Promise<Track>,
): QueueItem[] {
	const [tracks, setTracks] = useState<ReadonlyMap<string, Track>>(new Map());
	const idsKey = trackIds?.join(",") ?? "";
	useEffect(() => {
		const ids = idsKey ? [...new Set(idsKey.split(","))] : [];
		let cancelled = false;
		void (async () => {
			for (let at = 0; at < ids.length; at += TRACK_LOAD_CONCURRENCY) {
				const batch = ids.slice(at, at + TRACK_LOAD_CONCURRENCY);
				const loaded = await Promise.allSettled(batch.map(loadTrack));
				if (cancelled) return;
				setTracks((current) => {
					const next = new Map(current);
					for (const result of loaded) {
						if (result.status === "fulfilled") {
							next.set(result.value.id, result.value);
						}
					}
					return next;
				});
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [idsKey, loadTrack]);
	return useMemo(() => {
		const items: QueueItem[] = [];
		for (const [position, trackId] of (trackIds ?? []).entries()) {
			const track = tracks.get(trackId);
			if (!track) continue;
			items.push({
				id: roomQueueItemId(position, trackId),
				trackId,
				position,
				track,
				source: { kind: "user" },
			});
		}
		return items;
	}, [trackIds, tracks]);
}

/**
 * Listen Together for one client (ADR 0037): who it is to the Music Server,
 * the open Room, and the sync that keeps its Player on the Room State. Goes
 * around PlaybackProvider, which sends playback through the Room.
 */
export function ListenTogetherProvider({
	children,
	api,
	engine,
	shouldDriveEngine = true,
}: {
	children: ReactNode;
	api: ListenTogetherApi;
	engine: PlaybackEngine;
	/** One window drives a shared engine; the mini player only sends commands. */
	shouldDriveEngine?: boolean;
}) {
	const [role, setRole] = useState<ListenTogetherRole>("loading");
	const [isAvailable, setIsAvailable] = useState(false);
	const [room, setRoom] = useState<ListenTogetherRoom | null>(null);
	const [ending, setEnding] = useState<ListenTogetherEnding>(null);
	const apiRef = useRef(api);
	const roleRef = useRef(role);
	const roomRef = useRef(room);
	const newestRevisionRef = useRef<number | null>(null);
	const driverRef = useRef<RoomSyncDriver | null>(null);
	useLayoutEffect(() => {
		apiRef.current = api;
		roleRef.current = role;
	});

	const adoptRoom = useCallback((next: ListenTogetherRoom) => {
		if (isStaleSnapshot(next, newestRevisionRef.current)) return;
		newestRevisionRef.current = next.revision;
		const merged = {
			...next,
			// The Invite Link comes only in some of the Owner's responses.
			inviteUrl: next.inviteUrl ?? roomRef.current?.inviteUrl,
		};
		roomRef.current = merged;
		setRoom(merged);
	}, []);

	const endRoom = useCallback((reason: Exclude<ListenTogetherEnding, null>) => {
		if (!roomRef.current) return;
		const isGuest = roleRef.current === "guest";
		driverRef.current?.finish(isGuest ? "stop" : "pause");
		driverRef.current = null;
		roomRef.current = null;
		newestRevisionRef.current = null;
		setRoom(null);
		setEnding(reason);
		if (isGuest) {
			roleRef.current = "none";
			setRole("none");
		}
	}, []);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			const [sessionRole, health] = await Promise.all([
				fetchRole(apiRef.current),
				apiRef.current.getHealth().catch(() => null),
			]);
			const hasCapability = hasServerCapability(
				health,
				LISTEN_TOGETHER_CAPABILITY,
			);
			// Every Guest Listener advertises the capability. A health answer
			// without it means the Owner's, even on an older Music Server, which
			// answers the unknown session route with the Web Client's page. No
			// answer at all proves nothing, so it shows a visitor nothing.
			const nextRole =
				sessionRole ?? (health && !hasCapability ? "owner" : "none");
			const openRoom =
				nextRole === "guest" || (nextRole === "owner" && hasCapability)
					? await fetchRoom(apiRef.current)
					: null;
			if (cancelled) return;
			roleRef.current = nextRole;
			setIsAvailable(hasCapability);
			setRole(nextRole);
			if (openRoom) adoptRoom(openRoom);
		})();
		return () => {
			cancelled = true;
		};
	}, [adoptRoom]);

	// The page tells the Owner to restart the Music Server and come back.
	useEffect(() => {
		if (role !== "owner" || isAvailable) return undefined;
		let cancelled = false;
		const recheck = async () => {
			const health = await apiRef.current.getHealth().catch(() => null);
			if (!hasServerCapability(health, LISTEN_TOGETHER_CAPABILITY)) return;
			const openRoom = await fetchRoom(apiRef.current);
			if (cancelled) return;
			setIsAvailable(true);
			if (openRoom) adoptRoom(openRoom);
		};
		const onFocus = () => void recheck();
		window.addEventListener("focus", onFocus);
		return () => {
			cancelled = true;
			window.removeEventListener("focus", onFocus);
		};
	}, [role, isAvailable, adoptRoom]);

	const hasRoom = room !== null;

	useEffect(() => {
		if (!hasRoom) return undefined;
		return apiRef.current.subscribeListenTogetherRoomEvents(
			(event) => {
				if (event.type === "room-closed") endRoom("closed");
				else adoptRoom(event.room);
			},
			(error) => console.warn("Room event stream error", { error }),
		);
	}, [hasRoom, adoptRoom, endRoom]);

	const loadTrack = useMemo(() => createTrackCache(api.getTrack), [api]);
	const queue = useRoomQueue(room?.trackIds ?? null, loadTrack);

	const sendCommand = useCallback(
		async (command: ListenTogetherCommand) => {
			try {
				adoptRoom(await apiRef.current.sendListenTogetherCommand(command));
			} catch (error) {
				if (isListenTogetherRoomGone(error)) {
					endRoom("closed");
				} else if (error instanceof ApiError && error.status === 429) {
					toast.error("Too many changes at once. Try again in a moment.");
				} else {
					console.warn("Failed to send a Room command", { error });
					toast.error("The Room did not take that. Try again.");
				}
			}
		},
		[adoptRoom, endRoom],
	);

	const resolveSource = useCallback(
		async (trackId: string, queueItemId: string): Promise<PlaybackSource> => {
			const track = await loadTrack(trackId);
			return {
				type: "track",
				track,
				playbackUrl: apiRef.current.getTrackStreamUrl(trackId),
				queueItemId,
				artworkUrl: apiRef.current.getAlbumCoverUrl(track.albumId, 512),
			};
		},
		[loadTrack],
	);

	useEffect(() => {
		const openRoom = roomRef.current;
		if (!hasRoom || !shouldDriveEngine || !openRoom) return undefined;
		driverRef.current ??= new RoomSyncDriver({
			engine,
			getClock: () => apiRef.current.getListenTogetherClock(),
			resolveSource,
			sendCommand,
		});
		const driver = driverRef.current;
		driver.start(openRoom);
		return () => driver.suspend();
	}, [hasRoom, shouldDriveEngine, engine, resolveSource, sendCommand]);

	useEffect(() => {
		if (room) driverRef.current?.update(room);
	}, [room]);

	const openRoom = useCallback(async () => {
		setEnding(null);
		newestRevisionRef.current = null;
		try {
			adoptRoom(await apiRef.current.openListenTogetherRoom());
		} catch (error) {
			// Another of the Owner's clients opened it first; there is one Room.
			if (!(error instanceof ApiError && error.status === 409)) throw error;
			adoptRoom(await apiRef.current.getListenTogetherRoom());
		}
	}, [adoptRoom]);

	const closeRoom = useCallback(async () => {
		try {
			await apiRef.current.closeListenTogetherRoom();
		} catch (error) {
			if (!isListenTogetherRoomGone(error)) throw error;
		}
		endRoom("closed");
	}, [endRoom]);

	const leaveRoom = useCallback(async () => {
		try {
			await apiRef.current.leaveListenTogetherRoom();
		} catch (error) {
			if (!isListenTogetherRoomGone(error)) throw error;
		}
		endRoom("left");
	}, [endRoom]);

	const joinRoom = useCallback(
		async (token: string) => {
			// Before the first await: this call still has the click's permission.
			engine.unlockAudio?.();
			const joined = await apiRef.current.joinListenTogetherRoom(token);
			newestRevisionRef.current = null;
			roleRef.current = "guest";
			setRole("guest");
			setEnding(null);
			adoptRoom(joined);
		},
		[adoptRoom, engine],
	);

	const value = useMemo<ListenTogetherContextValue>(
		() => ({
			role,
			isAvailable,
			room,
			ending,
			openRoom,
			closeRoom,
			leaveRoom,
			joinRoom,
		}),
		[role, isAvailable, room, ending, openRoom, closeRoom, leaveRoom, joinRoom],
	);

	const roomPlayback = useMemo<RoomPlayback>(
		() => ({ role, room, queue, sendCommand }),
		[role, room, queue, sendCommand],
	);

	return (
		<ListenTogetherContext.Provider value={value}>
			<RoomPlaybackContext.Provider value={roomPlayback}>
				{children}
			</RoomPlaybackContext.Provider>
		</ListenTogetherContext.Provider>
	);
}
