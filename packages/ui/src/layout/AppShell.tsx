import { type CSSProperties, type ReactNode, useRef } from "react";
import { getQueuePanel } from "../widgets/layout-utils";
import { useLayout } from "./LayoutProvider";
import { QueueDrawer } from "./QueueDrawer";
import { PhoneShell } from "./PhoneShell";
import { QueuePinProvider, useQueuePin } from "./QueuePinProvider";
import { Sidebar } from "./Sidebar";
import {
	PLAYER_BAR_HEIGHT_PX,
	QUEUE_DRAWER_CONTENT_CLEARANCE,
} from "./shell-metrics";
import { Toaster } from "./Toaster";
import { TopNav, type WindowControls } from "./TopNav";
import { TopNavActionsProvider } from "./TopNavActions";
import { usePhoneShell } from "./use-phone-shell";
import { useQueueDrawerWidth } from "./use-queue-drawer-width";
import { WidgetDndProvider } from "./WidgetDock";

export { PLAYER_BAR_HEIGHT_PX } from "./shell-metrics";

// Docked Player Bar: scrolled content never ends hidden under the bar.
const PLAYER_DOCK_CONTENT_PADDING = "pb-(--player-dock-clearance)";
const PLAYER_DOCK_CLEARANCE = `${PLAYER_BAR_HEIGHT_PX}px`;

/**
 * App Shell: the top bar (Home and search) across the top, the Sidebar with
 * the library sections on the left, the page beside it, the Player Bar
 * docked along the bottom and the Queue Drawer on the right. While the
 * drawer is open and pinned the page gives way to it; unpinned it floats
 * over the page. Phones get the Phone Shell instead (ADR 0041); with no
 * QueuePinProvider there, the Queue shows no Pin.
 */
export function AppShell(props: AppShellProps) {
	const isPhone = usePhoneShell();
	if (isPhone) {
		return (
			<TopNavActionsProvider>
				<PhoneShell {...props} />
			</TopNavActionsProvider>
		);
	}
	return (
		<QueuePinProvider>
			<TopNavActionsProvider>
				<AppShellFrame {...props} />
			</TopNavActionsProvider>
		</QueuePinProvider>
	);
}

type AppShellProps = {
	children?: ReactNode;
	bottom?: ReactNode;
	/** What the host lists under the Sidebar's Playlists link. */
	sidebarPlaylists?: ReactNode;
	/** Opens the host's library search from the top bar's search field. */
	onSearch?: () => void;
	/** Minimize/maximize/close in the Top Nav, for a host without a title bar. */
	windowControls?: WindowControls;
};

function AppShellFrame({
	children,
	bottom,
	sidebarPlaylists,
	onSearch,
	windowControls,
}: AppShellProps) {
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
						// On the shell, so the Top Nav and the page share it.
						"--queue-drawer-clearance": QUEUE_DRAWER_CONTENT_CLEARANCE,
						"--library-search-width": metrics && `${metrics.searchWidth}px`,
						"--library-search-center": metrics && `${metrics.searchCenter}px`,
						"--library-search-top": metrics && `${metrics.searchTop}px`,
						"--library-search-row-height":
							metrics?.searchRowHeight !== undefined
								? `${metrics.searchRowHeight}px`
								: undefined,
					} as CSSProperties
				}
				data-app-shell
				data-queue-visible={
					preferences.layout.collapsed[queuePanel] ? undefined : ""
				}
				className="relative flex h-full min-h-0 w-full min-w-3xl flex-1 flex-col overflow-hidden bg-background text-foreground"
			>
				<TopNav onSearch={onSearch} windowControls={windowControls} />
				<div
					data-shell-inset-probe
					aria-hidden
					className="pointer-events-none invisible absolute w-(--shell-inset,2rem)"
				/>
				<div
					className="relative flex h-full min-h-0 min-w-0 flex-1 overflow-hidden"
					style={
						{
							"--player-dock-clearance": bottom
								? PLAYER_DOCK_CLEARANCE
								: undefined,
						} as CSSProperties
					}
				>
					<Sidebar playlists={sidebarPlaylists} />
					<main
						data-queue-open={queueTakesSpace ? "" : undefined}
						className={`flex h-full min-w-0 flex-1 flex-col overflow-auto bg-background pr-0 data-[queue-open]:pr-(--queue-drawer-clearance) transition-padding duration-300 ease-out ${
							bottom ? PLAYER_DOCK_CONTENT_PADDING : ""
						}`}
					>
						{children}
					</main>
					{bottom ? (
						<div
							data-player-dock
							className="pointer-events-none absolute inset-x-0 bottom-0 z-50"
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
