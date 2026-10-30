import { Link } from "@tanstack/react-router";
import {
	Disc3,
	Headphones,
	ListMusic,
	Music2,
	Radio,
	Settings,
	Tags,
	Users,
} from "lucide-react";
import { useContext } from "react";
import { ListenTogetherContext } from "../listen-together/ListenTogetherContext";
import { type GuestHiddenFeature, isHiddenFromRole } from "./guest-access";
import { NAV_LINK_CLASS } from "./TopNav";

/** Below `lg` a link is only its icon, which stays where it was. */
const RAIL_LINK_CLASS = "max-lg:w-11";
const NAV_ICON_CLASS = "size-5 shrink-0";

const libraryNav: {
	to: string;
	label: string;
	icon: typeof Disc3;
	hiddenFromGuest?: GuestHiddenFeature;
}[] = [
	{ to: "/library/albums", label: "Albums", icon: Disc3 },
	{ to: "/library/artists", label: "Artists", icon: Users },
	{ to: "/library/genres", label: "Genres", icon: Tags },
	{ to: "/radio", label: "Radio", icon: Radio, hiddenFromGuest: "radio" },
	{ to: "/library/tracks", label: "Tracks", icon: Music2 },
	{
		to: "/playlists",
		label: "Playlists",
		icon: ListMusic,
		hiddenFromGuest: "playlists",
	},
	{
		to: "/listen-together",
		label: "Listen Together",
		icon: Headphones,
		hiddenFromGuest: "listen-together-page",
	},
];

/**
 * The library sections down the left edge, Settings at the bottom. Docked
 * like the Queue Drawer: one line toward the page, no box. Below `lg` only
 * the icons stay, so the page keeps its room at the main target width.
 */
export function Sidebar() {
	const listenTogether = useContext(ListenTogetherContext);
	const role = listenTogether?.role;
	const isRoomOpen = role === "owner" && Boolean(listenTogether?.room);
	const visibleNav = libraryNav.filter(
		({ hiddenFromGuest }) =>
			!hiddenFromGuest || !isHiddenFromRole(role, hiddenFromGuest),
	);
	return (
		<aside
			data-sidebar
			data-shell-surface
			className="flex w-62.5 shrink-0 flex-col justify-between border-sidebar-border border-r bg-sidebar px-3 pt-6 pb-(--player-dock-clearance,0.75rem) text-sidebar-foreground max-lg:w-17"
		>
			<nav aria-label="Library" className="flex flex-col gap-1">
				{visibleNav.map(({ to, label, icon: Icon }) => {
					const marksRoom = isRoomOpen && to === "/listen-together";
					return (
						<Link
							key={to}
							to={to}
							title={marksRoom ? `${label}: Room open` : label}
							className={`${NAV_LINK_CLASS} ${RAIL_LINK_CLASS} relative`}
						>
							<Icon className={NAV_ICON_CLASS} />
							<span className="max-lg:sr-only">{label}</span>
							{marksRoom ? <RoomOpenMark /> : null}
						</Link>
					);
				})}
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

/** Beside the label, or on the icon's corner in the narrow rail. */
function RoomOpenMark() {
	return (
		<span className="ml-auto flex items-center max-lg:absolute max-lg:top-2 max-lg:right-2 max-lg:ml-0">
			<span aria-hidden className="size-2 rounded-full bg-primary" />
			<span className="sr-only">Room open</span>
		</span>
	);
}
