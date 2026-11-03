import { useSyncExternalStore } from "react";

/**
 * Whether the Music Server last answered that this client must sign in
 * (auth.session.v1; ADR 0042). Any request can find out, so it lives outside
 * React; the sign-in gate reads it.
 */
let signedOut = false;
const listeners = new Set<() => void>();

function setSignedOut(value: boolean) {
	if (signedOut === value) return;
	signedOut = value;
	for (const listener of listeners) listener();
}

export const markSignedOut = () => setSignedOut(true);
export const markSignedIn = () => setSignedOut(false);

function subscribe(listener: () => void) {
	listeners.add(listener);
	return () => listeners.delete(listener);
}

export function useSignedOut(): boolean {
	return useSyncExternalStore(subscribe, () => signedOut);
}
