import type { ListenTogetherRoom } from "@repo/api-client";
import { createContext, useContext } from "react";

/** "loading" until the Music Server has said who this client is (ADR 0037). */
export type ListenTogetherRole = "loading" | "owner" | "guest" | "none";

/** Why this client is no longer in a Room. */
export type ListenTogetherEnding = "closed" | "left" | null;

export type ListenTogetherContextValue = {
	role: ListenTogetherRole;
	/** The Music Server advertises `listen-together.v1`. */
	isAvailable: boolean;
	room: ListenTogetherRoom | null;
	ending: ListenTogetherEnding;
	openRoom(): Promise<void>;
	closeRoom(): Promise<void>;
	leaveRoom(): Promise<void>;
	/** Call from a click, so the browser allows audio. Rejects with the API error. */
	joinRoom(token: string): Promise<void>;
};

export const ListenTogetherContext =
	createContext<ListenTogetherContextValue | null>(null);

export function useListenTogether(): ListenTogetherContextValue {
	const value = useContext(ListenTogetherContext);
	if (value === null) {
		throw new Error(
			"useListenTogether must be used within a ListenTogetherProvider",
		);
	}
	return value;
}
