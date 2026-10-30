import { toast, useListenTogether } from "@repo/ui";
import { useState } from "react";
import { Button } from "#/components/ui/button";

/** Tells a Guest whose library this is and offers the way out (ADR 0037). */
export function GuestBar() {
	const listenTogether = useListenTogether();
	const [isLeaving, setIsLeaving] = useState(false);
	if (listenTogether.role !== "guest") return null;

	const leave = async () => {
		setIsLeaving(true);
		try {
			await listenTogether.leaveRoom();
		} catch (error) {
			console.warn("Failed to leave the Room", { error });
			toast.error(
				"Couldn't leave the room. Check your connection, then retry.",
			);
			setIsLeaving(false);
		}
	};

	return (
		<section
			aria-label="Listen Together"
			className="flex shrink-0 items-center gap-2 border-border border-b px-(--shell-inset) py-1 text-sm"
		>
			<span className="truncate font-medium text-heading">Owner's library</span>
			<span aria-hidden className="text-caption">
				·
			</span>
			<Button
				type="button"
				variant="ghost"
				size="sm"
				disabled={isLeaving}
				onClick={() => void leave()}
			>
				{isLeaving ? "Leaving…" : "Leave"}
			</Button>
		</section>
	);
}
