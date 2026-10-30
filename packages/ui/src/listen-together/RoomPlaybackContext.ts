import type {
	ListenTogetherCommand,
	ListenTogetherRoom,
	QueueItem,
} from "@repo/api-client";
import { createContext, useContext } from "react";
import type { ListenTogetherRole } from "./ListenTogetherContext";

/**
 * What the playback layer needs from Listen Together. Null without a
 * ListenTogetherProvider, where playback works as it always did.
 */
export type RoomPlayback = {
	role: ListenTogetherRole;
	/** The open Room this client follows. */
	room: ListenTogetherRoom | null;
	/** The Room Queue, for the Tracks whose metadata has loaded. */
	queue: QueueItem[];
	/** Sends a Room command; a refusal is shown to the user, never thrown. */
	sendCommand: (command: ListenTogetherCommand) => Promise<void>;
};

export const RoomPlaybackContext = createContext<RoomPlayback | null>(null);

export function useRoomPlayback(): RoomPlayback | null {
	return useContext(RoomPlaybackContext);
}

/**
 * Only an Owner has a personal Queue and Playback Preferences on the Music
 * Server; a client on the Guest Listener is refused them.
 */
export function hasOwnerAccess(playback: RoomPlayback | null): boolean {
	return playback === null || playback.role === "owner";
}
