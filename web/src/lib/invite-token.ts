/**
 * The Invite Link's token lives only in this variable once read: it stays out
 * of the address bar, storage, logs and query strings (ADR 0037).
 */
let inviteToken: string | null = null;

/** Moves the token from the URL fragment to memory. */
export function captureInviteToken(): void {
	const { hash, pathname, search } = window.location;
	if (!hash) return;
	inviteToken = hash.slice(1) || null;
	window.history.replaceState(window.history.state, "", pathname + search);
}

export function getInviteToken(): string | null {
	return inviteToken;
}

export function clearInviteToken(): void {
	inviteToken = null;
}
