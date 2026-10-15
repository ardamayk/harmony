import { Link } from "@tanstack/react-router";
import { Minus, Search, Settings, Square, X } from "lucide-react";
import { SHELL_INSET_CLASS, TOP_NAV_HEIGHT_CLASS } from "./shell-metrics";
import { TopNavActionsSlot } from "./TopNavActions";

const libraryNav = [
	{ to: "/library/albums", label: "Albums" },
	{ to: "/library/artists", label: "Artists" },
	{ to: "/library/genres", label: "Genres" },
	{ to: "/radio", label: "Radio" },
	{ to: "/library/tracks", label: "Tracks" },
	{ to: "/playlists", label: "Playlists" },
] as const;

const LINK_BASE_CLASS =
	"flex items-center gap-1.5 whitespace-nowrap rounded-lg px-3 py-2 text-sidebar-foreground text-sm transition hover:bg-[var(--shell-active)] hover:text-[var(--shell-active-foreground)] focus-visible:bg-[var(--shell-active)] focus-visible:text-[var(--shell-active-foreground)] focus-visible:outline-none";
const WINDOW_CONTROL_CLASS =
	"inline-flex size-8 items-center justify-center rounded-lg text-sidebar-foreground transition hover:bg-[var(--shell-active)] hover:text-[var(--shell-active-foreground)] focus-visible:bg-[var(--shell-active)] focus-visible:outline-none";

/** Window actions for a host that draws no system title bar (Desktop Client). */
export type WindowControls = {
	minimize: () => void;
	toggleMaximize: () => void;
	close: () => void;
};

// The open section (and a pressed Top Nav control) gets a lit pill: a lighter
// surface, full-strength text and a soft top highlight with a drop shadow.
const NAV_LINK_CLASS = `${LINK_BASE_CLASS} [&.active]:bg-[color-mix(in_srgb,var(--heading)_12%,transparent)] [&.active]:font-semibold [&.active]:text-heading [&.active]:shadow-[inset_0_1px_0_color-mix(in_srgb,var(--heading)_10%,transparent),0_8px_20px_-12px_rgb(0_0_0/0.8)]`;

/** Icon control in the Top Nav; `aria-pressed` lights it like the open section. */
export const TOP_NAV_ICON_BUTTON_CLASS =
	"inline-flex size-9 shrink-0 items-center justify-center rounded-lg text-sidebar-foreground transition hover:bg-[var(--shell-active)] hover:text-[var(--shell-active-foreground)] focus-visible:bg-[var(--shell-active)] focus-visible:outline-none aria-pressed:bg-[color-mix(in_srgb,var(--heading)_12%,transparent)] aria-pressed:text-heading aria-pressed:shadow-[inset_0_1px_0_color-mix(in_srgb,var(--heading)_10%,transparent),0_8px_20px_-12px_rgb(0_0_0/0.8)] [&.active]:bg-[color-mix(in_srgb,var(--heading)_12%,transparent)] [&.active]:text-heading [&.active]:shadow-[inset_0_1px_0_color-mix(in_srgb,var(--heading)_10%,transparent),0_8px_20px_-12px_rgb(0_0_0/0.8)]";

/**
 * Top navigation bar: brand, library sections, the Search entry (opens the
 * host's library search) and the Settings gear. Its content shares the page
 * inset, and the divider spans only that content column so it lines up with
 * the page title and the Player Bar below. The Desktop Client draws no system
 * title bar, so the bar's empty space carries `data-tauri-drag-region` to move
 * (and double-click to maximize) the window; browsers ignore the attribute.
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
			data-tauri-drag-region
			className={`flex ${TOP_NAV_HEIGHT_CLASS} shrink-0 items-stretch bg-sidebar text-sidebar-foreground ${
				windowControls ? "pl-[var(--shell-inset,2rem)]" : SHELL_INSET_CLASS
			}`}
		>
			{/* Window controls sit at the window's own corner, not on the page
			    inset line; the content keeps the inset when there is room. */}
			<div
				data-tauri-drag-region
				className={`flex h-full w-full min-w-0 items-center gap-6 border-sidebar-border border-b ${
					windowControls
						? "mr-[max(0px,calc(var(--shell-inset,2rem)-7.5rem))]"
						: ""
				}`}
			>
				<Link
					to="/library/albums"
					className="display-title shrink-0 font-semibold text-[var(--shell-brand)] text-xl tracking-[-0.5px]"
				>
					Earthly Audio
				</Link>
				<nav
					data-tauri-drag-region
					aria-label="Library"
					className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [&::-webkit-scrollbar]:h-1 [&::-webkit-scrollbar-button]:hidden [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-foreground/25 hover:[&::-webkit-scrollbar-thumb]:bg-foreground/40 [&::-webkit-scrollbar-track]:bg-transparent"
				>
					{libraryNav.map((item) => (
						<Link key={item.to} to={item.to} className={NAV_LINK_CLASS}>
							{item.label}
						</Link>
					))}
					{onSearch ? (
						<button
							type="button"
							onClick={onSearch}
							className={LINK_BASE_CLASS}
						>
							<Search className="size-4" />
							Search
						</button>
					) : null}
				</nav>
				<div className="flex shrink-0 items-center gap-1">
					<TopNavActionsSlot />
					<Link
						to="/settings"
						title="Settings"
						aria-label="Settings"
						className={TOP_NAV_ICON_BUTTON_CLASS}
					>
						<Settings className="size-4" />
					</Link>
				</div>
			</div>
			{windowControls ? (
				<div
					role="group"
					aria-label="Window"
					className="flex w-[7.5rem] shrink-0 items-center justify-end gap-1 pr-2"
				>
					<button
						type="button"
						aria-label="Minimize"
						title="Minimize"
						className={WINDOW_CONTROL_CLASS}
						onClick={windowControls.minimize}
					>
						<Minus className="size-4" />
					</button>
					<button
						type="button"
						aria-label="Maximize"
						title="Maximize"
						className={WINDOW_CONTROL_CLASS}
						onClick={windowControls.toggleMaximize}
					>
						<Square className="size-3.5" />
					</button>
					<button
						type="button"
						aria-label="Close"
						title="Close"
						className={`${WINDOW_CONTROL_CLASS} hover:bg-destructive/80 hover:text-white focus-visible:bg-destructive/80 focus-visible:text-white`}
						onClick={windowControls.close}
					>
						<X className="size-4" />
					</button>
				</div>
			) : null}
		</header>
	);
}
