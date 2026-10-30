import { createFileRoute } from "@tanstack/react-router";
import { captureInviteToken } from "#/lib/invite-token";
import { JoinPage } from "./-join-page";

export const Route = createFileRoute("/join")({
	// Before anything renders, so the token leaves the address bar at once.
	beforeLoad: () => captureInviteToken(),
	component: JoinPage,
});
