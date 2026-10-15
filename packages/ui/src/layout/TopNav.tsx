import { Link } from "@tanstack/react-router";
import { Search, Settings } from "lucide-react";
import { SHELL_INSET_CLASS, TOP_NAV_HEIGHT_CLASS } from "./shell-metrics";

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
const NAV_LINK_CLASS = `${LINK_BASE_CLASS} [&.active]:bg-[var(--shell-active)] [&.active]:font-semibold [&.active]:text-[var(--shell-active-foreground)]`;

/**
 * Top navigation bar: brand, library sections, the Search entry (opens the
 * host's library search) and the Settings gear. Its content shares the page
 * inset, and the divider spans only that content column so it lines up with
 * the page title and the Player Bar below.
 */
export function TopNav({ onSearch }: { onSearch?: () => void }) {
	return (
		<header
			className={`flex ${TOP_NAV_HEIGHT_CLASS} shrink-0 items-stretch bg-sidebar text-sidebar-foreground ${SHELL_INSET_CLASS}`}
		>
			<div className="flex h-full w-full min-w-0 items-center gap-6 border-sidebar-border border-b">
				<Link
					to="/library/albums"
					className="display-title shrink-0 font-semibold text-[var(--shell-brand)] text-xl tracking-[-0.5px]"
				>
					Earthly Audio
				</Link>
				<nav
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
				<Link
					to="/settings"
					title="Settings"
					aria-label="Settings"
					className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg text-sidebar-foreground transition hover:bg-[var(--shell-active)] hover:text-[var(--shell-active-foreground)] [&.active]:text-[var(--shell-active-foreground)]"
				>
					<Settings className="size-4" />
				</Link>
			</div>
		</header>
	);
}
