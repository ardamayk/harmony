import { type CSSProperties, type ReactNode, useRef } from "react";
import { getQueuePanel } from "../widgets/layout-utils";
import { useLayout } from "./LayoutProvider";
import { QueueDrawer } from "./QueueDrawer";
import { QueuePinProvider, useQueuePin } from "./QueuePinProvider";
import {
	QUEUE_DRAWER_CONTENT_CLEARANCE,
	SHELL_INSET_CLASS,
} from "./shell-metrics";
import { Toaster } from "./Toaster";
import { TopNav } from "./TopNav";
import { useQueueDrawerWidth } from "./use-queue-drawer-width";
import { WidgetDndProvider } from "./WidgetDock";

export { PLAYER_BAR_HEIGHT_PX, PLAYER_BAR_INSET_PX } from "./shell-metrics";

// Floating player bar: 86px bar + 16px bottom inset + 8px breathing room, so
// scrolled content never ends hidden under the bar.
const PLAYER_DOCK_CONTENT_PADDING = "pb-[110px]";

/**
 * App Shell: Top Nav across the top, the page below it, the Player Bar
 * floating over the page's bottom edge, and the Queue Drawer sliding up on
 * the right. Nav, page and bar share one horizontal inset; while the drawer
 * is open and pinned the page gives way to it, unpinned it floats over the
 * page, and the nav and bar stay put either way.
 */
export function AppShell(props: AppShellProps) {
	return (
		<QueuePinProvider>
			<AppShellFrame {...props} />
		</QueuePinProvider>
	);
}

type AppShellProps = {
	children?: ReactNode;
	bottom?: ReactNode;
	/** Opens the host's library search; shown as "Search" in the Top Nav. */
	onSearch?: () => void;
};

function AppShellFrame({ children, bottom, onSearch }: AppShellProps) {
	const shellRef = useRef<HTMLDivElement>(null);
	const metrics = useQueueDrawerWidth(shellRef);
	const { preferences } = useLayout();
	const queuePanel = getQueuePanel(preferences.layout.sidebarPosition);
	const queuePin = useQueuePin();
	// `data-queue-open` marks the page as making room for the drawer, which
	// only a pinned drawer asks for.
	const queueTakesSpace =
		!preferences.layout.collapsed[queuePanel] && (queuePin?.pinned ?? true);

	return (
		<WidgetDndProvider>
			<div
				ref={shellRef}
				style={
					{
						"--queue-drawer-width": metrics && `${metrics.queueWidth}px`,
						"--library-search-width": metrics && `${metrics.searchWidth}px`,
						"--library-search-center": metrics && `${metrics.searchCenter}px`,
					} as CSSProperties
				}
				data-app-shell
				className="relative flex h-full min-h-0 w-full flex-1 flex-col overflow-hidden bg-background text-foreground"
			>
				<TopNav onSearch={onSearch} />
				<div className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
					<main
						data-queue-open={queueTakesSpace ? "" : undefined}
						className={`flex h-full min-w-0 flex-1 flex-col overflow-auto bg-background pr-0 data-[queue-open]:pr-[var(--queue-drawer-clearance)] transition-[padding] duration-300 ease-out ${
							bottom ? PLAYER_DOCK_CONTENT_PADDING : ""
						}`}
						style={
							{
								"--queue-drawer-clearance": QUEUE_DRAWER_CONTENT_CLEARANCE,
							} as CSSProperties
						}
					>
						{children}
					</main>
					{bottom ? (
						<div
							data-player-scrim
							aria-hidden
							className="pointer-events-none absolute inset-x-0 bottom-0 z-20 h-36 bg-gradient-to-t from-background via-background/75 to-transparent"
						/>
					) : null}
					{bottom ? (
						<div
							data-player-dock
							className={`pointer-events-none absolute inset-x-0 bottom-4 z-50 ${SHELL_INSET_CLASS}`}
						>
							<div data-player-dock-column className="w-full">
								<div className="pointer-events-auto">{bottom}</div>
							</div>
						</div>
					) : null}
				</div>
				<QueueDrawer abovePlayerBar={Boolean(bottom)} />
				<Toaster />
			</div>
		</WidgetDndProvider>
	);
}
