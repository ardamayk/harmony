import { Link } from "@tanstack/react-router";
import { House, Minus, Search, Square, X } from "lucide-react";
import { TOP_NAV_HEIGHT_CLASS } from "./shell-metrics";
import { TopNavActionsSlot } from "./TopNavActions";

const LINK_BASE_CLASS =
	"flex items-center gap-2 whitespace-nowrap rounded-md px-3 py-2 text-sidebar-foreground text-sm transition hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading focus-visible:outline-none";
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
 * Top bar: the brand over the Sidebar, Home and the library search field on
 * the page's content line, page actions on the right. The field opens the host's search,
 * whose card drops down over it (`data-library-search-field` is what the
 * card lines up with). The Desktop Client draws no system title bar, so the
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
			className={`flex ${TOP_NAV_HEIGHT_CLASS} shrink-0 items-center border-sidebar-border border-b bg-sidebar text-sidebar-foreground ${
				windowControls ? "" : "pr-6"
			}`}
		>
			{/* As wide as the Sidebar, so the search centres on the page rather
			    than on the window. */}
			<div
				data-window-drag-region
				className="flex w-60 shrink-0 pl-6 max-lg:w-16 max-lg:pl-3"
			>
				<Link
					to="/library/albums"
					className="font-display shrink-0 font-semibold text-shell-brand text-xl tracking-tight max-lg:sr-only"
				>
					Earthly Audio
				</Link>
			</div>
			{/* On wide windows the search grows rightward only: the extra width
			    goes in as margin on its left, half of it moving the centre. */}
			<div
				data-window-drag-region
				className="min-w-0 flex-1 basis-0 xl:mr-24"
			/>
			<div className="flex w-full max-w-xl min-w-0 items-center gap-2 xl:max-w-2xl">
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
				// The Queue Drawer's width on this side keeps the search centred on
				// the page between Sidebar and drawer, whether or not it is open.
				// Narrower windows centre it on the bar, clear of the actions.
				className="flex min-w-0 flex-1 basis-0 items-center justify-end gap-1 xl:ml-(--queue-drawer-clearance)"
			>
				<TopNavActionsSlot />
				{windowControls ? (
					<WindowControlButtons controls={windowControls} className="pr-2" />
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
