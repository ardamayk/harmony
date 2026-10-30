import { ApiError } from "@repo/api-client";
import { useListenTogether } from "@repo/ui";
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { ListenTogetherNotice } from "#/components/listen-together-gate";
import { Button } from "#/components/ui/button";
import { clearInviteToken, getInviteToken } from "#/lib/invite-token";

export const INVITE_INVALID_MESSAGE =
	"This invite link is invalid or has expired";
export const ROOM_FULL_MESSAGE = "This room is full";
const JOIN_FAILED_MESSAGE =
	"Couldn't reach the Music Server. Check your connection, then retry.";

export function joinErrorMessage(error: unknown): string {
	const code = error instanceof ApiError ? error.body.code : undefined;
	switch (code) {
		case "invite_invalid":
			return INVITE_INVALID_MESSAGE;
		case "room_full":
			return ROOM_FULL_MESSAGE;
		default:
			return JOIN_FAILED_MESSAGE;
	}
}

/**
 * The Guest Listener's landing page. The click on Join is what lets the
 * browser play audio. The token never leaves memory except in the request
 * that spends it, so the failure below is shown and never logged.
 */
export function JoinPage() {
	const listenTogether = useListenTogether();
	const navigate = useNavigate();
	const [token] = useState(getInviteToken);
	const [isJoining, setIsJoining] = useState(false);
	const [error, setError] = useState<string | null>(null);

	if (!token) {
		return (
			<ListenTogetherNotice
				title="You need an invite link"
				description="Ask the person whose library this is for an Invite Link, then open it here."
			/>
		);
	}

	const join = async () => {
		setIsJoining(true);
		setError(null);
		try {
			await listenTogether.joinRoom(token);
			clearInviteToken();
			await navigate({ to: "/library/albums", replace: true });
		} catch (joinError) {
			setError(joinErrorMessage(joinError));
			setIsJoining(false);
		}
	};

	return (
		<ListenTogetherNotice
			title="You're invited to listen together"
			description="Join to hear the same music as everyone in this Room, from the Owner's library."
		>
			{error === INVITE_INVALID_MESSAGE ? null : (
				<Button
					type="button"
					size="lg"
					disabled={isJoining}
					onClick={() => void join()}
				>
					{isJoining ? "Joining…" : "Join"}
				</Button>
			)}
			{error ? (
				<p role="alert" className="text-destructive text-sm">
					{error}
				</p>
			) : null}
		</ListenTogetherNotice>
	);
}
