import { Link } from "@tanstack/react-router";
import {
	Disc3,
	ListMusic,
	Music2,
	Radio,
	Settings,
	Tags,
	Users,
} from "lucide-react";
import { NAV_LINK_CLASS } from "./TopNav";

/** Below `lg` a link is only its icon, which stays where it was. */
const RAIL_LINK_CLASS = "max-lg:w-11";
const NAV_ICON_CLASS = "size-5 shrink-0";

const libraryNav = [
	{ to: "/library/albums", label: "Albums", icon: Disc3 },
	{ to: "/library/artists", label: "Artists", icon: Users },
	{ to: "/library/genres", label: "Genres", icon: Tags },
	{ to: "/radio", label: "Radio", icon: Radio },
	{ to: "/library/tracks", label: "Tracks", icon: Music2 },
	{ to: "/playlists", label: "Playlists", icon: ListMusic },
] as const;

/**
 * The library sections down the left edge, Settings at the bottom. Docked
 * like the Queue Drawer: one line toward the page, no box. Below `lg` only
 * the icons stay, so the page keeps its room at the main target width.
 */
export function Sidebar() {
	return (
		<aside
			data-sidebar
			data-shell-surface
			className="flex w-62.5 shrink-0 flex-col justify-between border-sidebar-border border-r bg-sidebar px-3 pt-6 pb-(--player-dock-clearance,0.75rem) text-sidebar-foreground max-lg:w-17"
		>
			<nav aria-label="Library" className="flex flex-col gap-1">
				{libraryNav.map(({ to, label, icon: Icon }) => (
					<Link
						key={to}
						to={to}
						title={label}
						className={`${NAV_LINK_CLASS} ${RAIL_LINK_CLASS}`}
					>
						<Icon className={NAV_ICON_CLASS} />
						<span className="max-lg:sr-only">{label}</span>
					</Link>
				))}
			</nav>
			<Link
				to="/settings"
				title="Settings"
				className={`${NAV_LINK_CLASS} ${RAIL_LINK_CLASS} my-3`}
			>
				<Settings className={NAV_ICON_CLASS} />
				<span className="max-lg:sr-only">Settings</span>
			</Link>
		</aside>
	);
}
