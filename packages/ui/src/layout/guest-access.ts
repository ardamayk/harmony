import { useContext } from "react";
import {
	ListenTogetherContext,
	type ListenTogetherRole,
} from "../listen-together/ListenTogetherContext";

/**
 * Everything the Guest interface hides (ADR 0037). This is a convenience, not
 * security: the Music Server refuses all of it through its Guest allowlist.
 * To audit the interface, search for `useHiddenFromGuest`.
 */
export const GUEST_HIDDEN_FEATURES = [
	"album-deletion",
	"album-radio",
	"external-source",
	"favorites",
	"listen-together-page",
	"managed-import",
	"playlists",
	/** Queue changes the Room has no command for: add, remove, clear, shuffle, repeat. */
	"queue-editing",
	"radio",
	/** Settings about the Music Server, not this device. */
	"server-settings",
	"track-deletion",
	"track-replacement",
] as const;

export type GuestHiddenFeature = (typeof GUEST_HIDDEN_FEATURES)[number];

/** Routes a Guest is sent away from when opening them directly. */
export const GUEST_BLOCKED_ROUTE_PREFIXES = [
	"/favorites",
	"/listen-together",
	"/playlists",
	"/radio",
] as const;

export function isHiddenFromRole(
	role: ListenTogetherRole | undefined,
	_feature: GuestHiddenFeature,
): boolean {
	return role === "guest";
}

export function isGuestBlockedRoute(pathname: string): boolean {
	return GUEST_BLOCKED_ROUTE_PREFIXES.some(
		(prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`),
	);
}

/**
 * Whether the Guest interface hides a feature. Without a provider, as in a
 * component test, nothing is hidden: this is the Owner's interface.
 */
export function useHiddenFromGuest(feature: GuestHiddenFeature): boolean {
	const listenTogether = useContext(ListenTogetherContext);
	return isHiddenFromRole(listenTogether?.role, feature);
}
