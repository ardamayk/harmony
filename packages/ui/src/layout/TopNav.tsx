import { Link } from "@tanstack/react-router";
import { House, Minus, Search, Square, X } from "lucide-react";
import { TOP_NAV_HEIGHT_CLASS } from "./shell-metrics";
import { TopNavActionsSlot, TopNavBackdropSlot } from "./TopNavActions";

const LINK_BASE_CLASS =
	"flex h-10 items-center gap-2 whitespace-nowrap rounded-md px-3 text-sidebar-foreground text-sm transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none";
const WINDOW_CONTROL_CLASS =
	"inline-flex size-8 items-center justify-center rounded-md text-sidebar-foreground transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:outline-none";

/** Window actions for a host that draws no system title bar (Desktop Client). */
export type WindowControls = {
	minimize: () => void;
	toggleMaximize: () => void;
	close: () => void;
};

// The open section (and a pressed Top Nav control) gets the active pill: the
// active step, heading text, an accent icon and a soft top highlight with a
// drop shadow. Hover stays off the accent, so only the open section is marked.
export const NAV_LINK_CLASS = `${LINK_BASE_CLASS} [&.active]:bg-neutral-active [&.active]:font-semibold [&.active]:text-heading [&.active_svg]:text-shell-active-foreground [&.active]:shadow-lit`;

/** Icon control in the Top Nav; `aria-pressed` lights it like the open section. */
export const TOP_NAV_ICON_BUTTON_CLASS =
	"inline-flex size-9 shrink-0 items-center justify-center rounded-md text-sidebar-foreground transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:outline-none aria-pressed:bg-neutral-active aria-pressed:text-shell-active-foreground aria-pressed:shadow-lit [&.active]:bg-neutral-active [&.active]:text-shell-active-foreground [&.active]:shadow-lit";

const ROUND_CONTROL_CLASS =
	"border border-border bg-card text-sidebar-foreground transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none";

/**
 * Top bar: the brand on the left, Home and the library search field in the
 * window's centre at one width, page actions on the right. The field opens the
 * host's search, whose card drops down over it (`data-library-search-field` is
 * what the card lines up with), over the Queue Drawer when that is open. The
 * Desktop Client draws no system title bar, so the
 * bar's empty space carries `data-window-drag-region` to move (and
 * double-click to maximize) the window; browsers ignore the attribute.
 */
export function TopNav({
	onSearch,
	windowControls,
}: {
	onSearch?: () => void;
	windowControls?: WindowControls;
}) {
	return (
		<header
			data-shell-surface
			data-window-drag-region
			// Side tracks with one minimum keep the middle on the window's centre
			// at every width; it narrows only when the window cannot fit it.
			className={`relative grid ${TOP_NAV_HEIGHT_CLASS} shrink-0 grid-cols-[minmax(11.5rem,1fr)_minmax(0,42rem)_minmax(11.5rem,1fr)] lg:grid-cols-[minmax(16.25rem,1fr)_minmax(0,42rem)_minmax(16.25rem,1fr)] items-center gap-4 bg-sidebar text-sidebar-foreground`}
		>
			{/* First, so the columns below (all `relative`) paint over it. */}
			<TopNavBackdropSlot />
			{/* Continues the Sidebar's line to the top, so the two read as one
			    column while the search stays on the window's centre. */}
			<div
				aria-hidden
				className="pointer-events-none absolute inset-y-0 left-0 w-62.5 border-sidebar-border border-r max-lg:w-17"
			/>
			<div data-window-drag-region className="relative flex pl-6">
				<Link
					to="/library/albums"
					aria-label="Harmony"
					className="font-display flex shrink-0 items-center gap-2 font-semibold text-shell-brand text-xl tracking-tight"
				>
					<span aria-hidden className="brand-mark size-5 shrink-0 lg:size-6" />
					<span className="text-brand-gradient max-lg:hidden">Harmony</span>
				</Link>
			</div>
			<div className="relative flex min-w-0 items-center gap-2">
				<Link
					to="/"
					title="Home"
					aria-label="Home"
					// The icon alone, like the external links: no card around it;
					// hover and focus show the neutral step behind it.
					className="inline-flex size-10 shrink-0 items-center justify-center rounded-md text-sidebar-foreground transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none"
				>
					<House className="size-7" />
				</Link>
				{onSearch ? (
					<button
						type="button"
						data-library-search-field
						onClick={onSearch}
						className={`${ROUND_CONTROL_CLASS} flex h-12 min-w-0 flex-1 items-center gap-3 rounded-full px-4 text-left`}
					>
						<Search className="size-5 shrink-0" />
						<span className="truncate text-caption text-sm">
							Search your library
						</span>
					</button>
				) : null}
			</div>
			<div
				data-window-drag-region
				className={`relative flex items-center justify-end gap-1 ${
					windowControls ? "" : "pr-3.5"
				}`}
			>
				<TopNavActionsSlot />
				{windowControls ? (
					<WindowControlButtons controls={windowControls} className="pr-4" />
				) : null}
			</div>
		</header>
	);
}

/** Minimize, maximize and close for a window without a system title bar. */
export function WindowControlButtons({
	controls,
	className = "",
}: {
	controls: WindowControls;
	className?: string;
}) {
	return (
		<div
			role="group"
			aria-label="Window"
			className={`flex shrink-0 items-center justify-end gap-1 ${className}`}
		>
			<button
				type="button"
				aria-label="Minimize"
				title="Minimize"
				className={WINDOW_CONTROL_CLASS}
				onClick={controls.minimize}
			>
				<Minus className="size-4" />
			</button>
			<button
				type="button"
				aria-label="Maximize"
				title="Maximize"
				className={WINDOW_CONTROL_CLASS}
				onClick={controls.toggleMaximize}
			>
				<Square className="size-3.5" />
			</button>
			<button
				type="button"
				aria-label="Close"
				title="Close"
				className={`${WINDOW_CONTROL_CLASS} hover:bg-destructive/80 hover:text-destructive-foreground focus-visible:bg-destructive/80 focus-visible:text-destructive-foreground`}
				onClick={controls.close}
			>
				<X className="size-4" />
			</button>
		</div>
	);
}
