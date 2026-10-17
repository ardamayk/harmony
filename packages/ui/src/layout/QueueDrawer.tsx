import type { CSSProperties } from "react";
import { cn } from "../lib/utils";
import { getQueuePanel } from "../widgets/layout-utils";
import { useLayout } from "./LayoutProvider";
import { QueuePanel } from "./QueuePanel";
import {
	QUEUE_DRAWER_BOTTOM,
	QUEUE_DRAWER_BOTTOM_ABOVE_PLAYER,
	QUEUE_DRAWER_TOP,
} from "./shell-metrics";

/**
 * The Queue as a card that slides up from the bottom edge and parks between
 * the Top Nav and the Player Bar, its right edge on the page inset line. Open
 * state is the Queue panel's `collapsed` preference, so the Player Bar's queue
 * button and this card's close button drive the same flag.
 */
export function QueueDrawer({ abovePlayerBar }: { abovePlayerBar: boolean }) {
	const { preferences, togglePanel } = useLayout();
	const queuePanel = getQueuePanel(preferences.layout.sidebarPosition);
	const open = !preferences.layout.collapsed[queuePanel];

	return (
		<aside
			data-queue-drawer
			data-shell-surface
			data-state={open ? "open" : "closed"}
			aria-label="Queue"
			aria-hidden={!open}
			inert={!open}
			className={cn(
				"absolute top-(--queue-drawer-top) right-(--shell-inset,2rem) bottom-(--queue-drawer-bottom) z-40 w-(--queue-drawer-width,18rem) max-w-queue-drawer-max overflow-hidden rounded-lg border border-player-border shadow-shell transition-slide duration-300 ease-out",
				// Closed: slides below the viewport, then goes fully invisible so
				// it can never peek out under the Player Bar, whatever the host
				// window's geometry does to the shell.
				open ? "visible translate-y-0" : "invisible translate-y-below-dock",
			)}
			style={
				{
					"--queue-drawer-top": QUEUE_DRAWER_TOP,
					"--queue-drawer-bottom": abovePlayerBar
						? QUEUE_DRAWER_BOTTOM_ABOVE_PLAYER
						: QUEUE_DRAWER_BOTTOM,
				} as CSSProperties
			}
		>
			<div className="relative flex h-full w-full flex-col overflow-hidden bg-player text-player-foreground">
				<QueuePanel embedded onClose={() => togglePanel(queuePanel)} />
			</div>
		</aside>
	);
}
