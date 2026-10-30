import { isGuestBlockedRoute } from "@repo/ui/guest-access";
import {
	type ListenTogetherEnding,
	type ListenTogetherRole,
	ThemeProvider,
	useListenTogether,
} from "@repo/ui";
import { Navigate, Outlet, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { readGuestPreferences } from "#/lib/guest-preferences";

const JOIN_PATH = "/join";
const GUEST_HOME = "/library/albums";
const OWNER_JOIN_TARGET = "/listen-together";

export type ListenTogetherView =
	| { kind: "app" }
	| { kind: "blank" }
	| { kind: "join" }
	| { kind: "needs-invite" }
	| { kind: "ended"; ending: "closed" | "left" }
	| { kind: "redirect"; to: typeof GUEST_HOME | typeof OWNER_JOIN_TARGET };

/** What the shell shows for this client's place in Listen Together (ADR 0037). */
export function resolveListenTogetherView(
	role: ListenTogetherRole,
	ending: ListenTogetherEnding,
	pathname: string,
): ListenTogetherView {
	const isJoinPath = pathname === JOIN_PATH || pathname === `${JOIN_PATH}/`;
	// The Owner's library outlives a Room: closing it ends nothing for them.
	if (role === "owner") {
		return isJoinPath
			? { kind: "redirect", to: OWNER_JOIN_TARGET }
			: { kind: "app" };
	}
	if (ending) return { kind: "ended", ending };
	if (role === "loading") return { kind: "blank" };
	if (role === "none") {
		return isJoinPath ? { kind: "join" } : { kind: "needs-invite" };
	}
	return isJoinPath || isGuestBlockedRoute(pathname)
		? { kind: "redirect", to: GUEST_HOME }
		: { kind: "app" };
}

const ENDING_NOTICES = {
	closed: {
		title: "The room was closed",
		description: "Ask for a new Invite Link to listen together again.",
	},
	left: {
		title: "You left the room",
		description: "Open an Invite Link to listen together again.",
	},
} as const;

/**
 * Decides what this client may see before the library mounts, so a client
 * without a Guest session makes no library request, sees no flash of the
 * library, and is never offered it after a Room ends.
 */
export function ListenTogetherGate({ children }: { children: ReactNode }) {
	const { role, ending } = useListenTogether();
	const pathname = useRouterState({
		select: (state) => state.location.pathname,
	});
	const view = resolveListenTogetherView(role, ending, pathname);
	switch (view.kind) {
		case "app":
			return children;
		case "join":
			return <Outlet />;
		case "blank":
			return <div aria-busy className="min-h-0 flex-1 bg-background" />;
		case "redirect":
			return <Navigate to={view.to} replace />;
		case "needs-invite":
			return (
				<ListenTogetherNotice
					title="You need an invite link"
					description="Ask the person whose library this is for an Invite Link, then open it here."
				/>
			);
		case "ended":
			return <ListenTogetherNotice {...ENDING_NOTICES[view.ending]} />;
	}
}

/** A whole-window message for a client that is not in the library. */
export function ListenTogetherNotice({
	title,
	description,
	children,
}: {
	title: string;
	description: string;
	children?: ReactNode;
}) {
	return (
		<main className="flex h-full min-h-0 flex-1 flex-col items-center justify-center gap-6 bg-background p-8 text-center text-foreground">
			{/* The library's ThemeSync is not mounted out here. */}
			<ThemeProvider theme={readGuestPreferences().theme} />
			<div className="flex max-w-prose flex-col gap-2">
				<h1 className="font-semibold text-2xl text-heading">{title}</h1>
				<p className="text-caption text-sm">{description}</p>
			</div>
			{children}
		</main>
	);
}
