import type { ListenTogetherRoom } from "@repo/api-client";
import { toast, useListenTogether } from "@repo/ui";
import { type MouseEvent, useState } from "react";
import { PageHeader, PageShell } from "#/components/page-layout";
import { Button } from "#/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogTitle,
} from "#/components/ui/dialog";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import {
	getServerConnection,
	isDesktopClient,
	openExternalUrl,
} from "#/desktop/bridge";
import { getApiBaseUrl } from "#/env";

const DOCS_PATH = "/docs/#listen-together";
const OPEN_FAILED_MESSAGE =
	"Couldn't open the Room. Check that the Music Server is running, then retry.";
const CLOSE_FAILED_MESSAGE =
	"Couldn't close the Room. Check that the Music Server is running, then retry.";

function formatExpiry(expiresAt: string): string {
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: "medium",
		timeStyle: "short",
	}).format(new Date(expiresAt));
}

export function ListenTogetherPage() {
	const { isAvailable, room } = useListenTogether();

	return (
		<PageShell
			testId="listen-together-page-shell"
			contentTestId="listen-together-page-content"
			header={<PageHeader title="Listen Together" />}
		>
			<div className="flex max-w-prose flex-col gap-8">
				{room ? (
					<OpenRoom room={room} />
				) : isAvailable ? (
					<OpenRoomAction />
				) : (
					<GuestListenerRequired />
				)}
				{isDesktopClient() ? <FriendLibraryPlaceholder /> : null}
			</div>
		</PageShell>
	);
}

function OpenRoomAction() {
	const listenTogether = useListenTogether();
	const [isOpening, setIsOpening] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const open = async () => {
		setIsOpening(true);
		setError(null);
		try {
			await listenTogether.openRoom();
		} catch (openError) {
			console.warn("Failed to open the Room", { error: openError });
			setError(OPEN_FAILED_MESSAGE);
		} finally {
			setIsOpening(false);
		}
	};

	return (
		<section className="flex flex-col items-start gap-4">
			<p className="text-foreground text-sm">
				Open a Room and share its Invite Link. Friends hear the same Track at
				the same time, each on their own device, and anyone can pause, seek or
				pick the next Track.
			</p>
			<Button
				type="button"
				size="lg"
				disabled={isOpening}
				onClick={() => void open()}
			>
				{isOpening ? "Opening…" : "Listen with friends in my library"}
			</Button>
			{error ? (
				<p role="alert" className="text-destructive text-sm">
					{error}
				</p>
			) : null}
		</section>
	);
}

/** The Desktop Client's page is local, so the docs come from its Music Server. */
function openDesktopDocs(event: MouseEvent<HTMLAnchorElement>) {
	if (!isDesktopClient()) return;
	event.preventDefault();
	void getServerConnection()
		.then((connection) =>
			openExternalUrl(`${connection?.origin ?? ""}${DOCS_PATH}`),
		)
		.catch((error) => {
			console.warn("Failed to open the docs", { error });
			toast.error("Couldn't open the docs. Open /docs on the Music Server.");
		});
}

function GuestListenerRequired() {
	return (
		<section className="flex flex-col gap-2">
			<h2 className="font-medium text-heading text-sm">
				Set up a Guest Listener first
			</h2>
			<p className="text-foreground text-sm">
				Friends need an address that reaches this Music Server. Set GUEST_ADDR
				and PUBLIC_URL on the Music Server, restart it, then come back here.
			</p>
			<a
				href={`${getApiBaseUrl()}${DOCS_PATH}`}
				target="_blank"
				rel="noreferrer"
				onClick={openDesktopDocs}
				className="self-start text-foreground text-sm underline underline-offset-4 hover:text-heading focus-visible:text-heading phone:inline-flex phone:min-h-11 phone:items-center"
			>
				Read how to set up Listen Together
			</a>
		</section>
	);
}

function OpenRoom({ room }: { room: ListenTogetherRoom }) {
	const inviteUrl = room.inviteUrl ?? "";

	const copyInviteLink = async () => {
		try {
			await navigator.clipboard.writeText(inviteUrl);
			toast.success("Invite Link copied");
		} catch {
			toast.error("Couldn't copy the Invite Link. Select it and copy it.");
		}
	};

	return (
		<>
			<section className="flex flex-col gap-2">
				<Label htmlFor="invite-link">Invite Link</Label>
				<div className="flex items-center gap-2">
					<Input
						id="invite-link"
						readOnly
						value={inviteUrl}
						onFocus={(event) => event.currentTarget.select()}
					/>
					<Button
						type="button"
						variant="outline"
						disabled={!inviteUrl}
						onClick={() => void copyInviteLink()}
					>
						Copy
					</Button>
				</div>
				<p className="text-caption text-sm">
					Anyone with this link joins until it expires, so share it like a
					password.
				</p>
			</section>
			<dl className="flex flex-col gap-2 text-sm">
				<div className="flex gap-2">
					<dt className="text-caption">Guests</dt>
					<dd className="text-foreground tabular-nums">
						{room.guestCount} of {room.maxGuests}
					</dd>
				</div>
				<div className="flex gap-2">
					<dt className="text-caption">Expires</dt>
					<dd className="text-foreground">{formatExpiry(room.expiresAt)}</dd>
				</div>
			</dl>
			<CloseRoomAction />
		</>
	);
}

function CloseRoomAction() {
	const listenTogether = useListenTogether();
	const [isConfirming, setIsConfirming] = useState(false);
	const [isClosing, setIsClosing] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const close = async () => {
		setIsClosing(true);
		setError(null);
		try {
			await listenTogether.closeRoom();
			setIsConfirming(false);
		} catch (closeError) {
			console.warn("Failed to close the Room", { error: closeError });
			setError(CLOSE_FAILED_MESSAGE);
		} finally {
			setIsClosing(false);
		}
	};

	return (
		<div>
			<Button
				type="button"
				variant="outline"
				onClick={() => setIsConfirming(true)}
			>
				Close room
			</Button>
			<Dialog
				open={isConfirming}
				onOpenChange={(open) => {
					if (isClosing) return;
					setIsConfirming(open);
					setError(null);
				}}
			>
				<DialogContent>
					<DialogTitle>Close this Room?</DialogTitle>
					<DialogDescription className="mt-2">
						Every Guest leaves at once and the Invite Link stops working. Your
						own Queue stays as it was.
					</DialogDescription>
					{error ? (
						<p role="alert" className="mt-4 text-destructive text-sm">
							{error}
						</p>
					) : null}
					<DialogFooter className="mt-6">
						<Button
							type="button"
							variant="outline"
							disabled={isClosing}
							onClick={() => setIsConfirming(false)}
						>
							Keep room open
						</Button>
						<Button
							type="button"
							variant="destructive"
							disabled={isClosing}
							onClick={() => void close()}
						>
							{isClosing ? "Closing…" : "Close room"}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</div>
	);
}

/** Joining from the Desktop Client is not built (ADR 0037): no handler, no code. */
function FriendLibraryPlaceholder() {
	return (
		<section className="flex flex-col items-start gap-2">
			<Button type="button" variant="outline" disabled>
				Connect to a friend's library
			</Button>
			<p className="text-caption text-sm">
				Not available yet. For now, friends join from a browser.
			</p>
		</section>
	);
}
