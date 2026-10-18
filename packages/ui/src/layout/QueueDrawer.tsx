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
 * The Queue as a column docked on the right window edge, between the Top Nav
 * and the Player Bar, set off from the page by one line; it slides in from
 * the right. Open state is the Queue panel's `collapsed` preference, so the Player Bar's queue
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
				"absolute top-(--queue-drawer-top) right-0 bottom-(--queue-drawer-bottom) z-40 w-(--queue-drawer-width,18rem) max-w-queue-drawer-max overflow-hidden border-player-border border-l transition-slide duration-300 ease-out",
				// Closed: slides past the right edge, then goes fully invisible.
				open ? "visible translate-x-0" : "invisible translate-x-full",
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
