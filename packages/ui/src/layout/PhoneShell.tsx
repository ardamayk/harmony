import { Link, useLocation } from "@tanstack/react-router";
import { Ellipsis, ListMusic, Search, X } from "lucide-react";
import {
	type ReactNode,
	useCallback,
	useContext,
	useEffect,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { useFocusTrap } from "../lib/use-focus-trap";
import { cn } from "../lib/utils";
import {
	ListenTogetherContext,
	type ListenTogetherRole,
} from "../listen-together/ListenTogetherContext";
import {
	favoritesNav,
	isNavLinkVisible,
	libraryNav,
	playlistsNav,
	RoomOpenMark,
	settingsNav,
} from "./Sidebar";
import { QueuePanel } from "./QueuePanel";
import { Toaster } from "./Toaster";
import { NAV_LINK_CLASS, TOP_NAV_ICON_BUTTON_CLASS } from "./TopNav";
import { TopNavActionsSlot } from "./TopNavActions";
import { useCloseOnNavigate } from "./use-close-on-navigate";

const TAB_ROUTES = [
	"/library/albums",
	"/library/artists",
	"/playlists",
	"/radio",
];
const MORE_ROUTES = [
	"/library/tracks",
	"/library/genres",
	"/favorites",
	"/listen-together",
	"/settings",
];
const SHELL_NAV = [...libraryNav, playlistsNav, favoritesNav, settingsNav];

const TAB_CLASS = cn(
	NAV_LINK_CLASS,
	"h-11 min-w-0 flex-1 flex-col justify-center gap-0.5 px-0 text-xs",
);
const ICON_BUTTON_CLASS = cn(TOP_NAV_ICON_BUTTON_CLASS, "size-11");

/** What the tab bar and the More sheet offer this role, in display order. */
export function phoneNav(role: ListenTogetherRole | undefined) {
	const pick = (routes: string[]) =>
		routes.flatMap((route) =>
			SHELL_NAV.filter(
				(link) => link.to === route && isNavLinkVisible(link, role),
			),
		);
	return { tabs: pick(TAB_ROUTES), more: pick(MORE_ROUTES) };
}

/**
 * App Shell below the desktop shell's minimum width (ADR 0041): a slim top
 * bar, the page, the mini player (`bottom`) and the tab bar, stacked in flow so
 * nothing covers the page. Layout Preferences belong to the desktop, so this
 * shell neither reads nor writes them and has no Queue Drawer.
 */
export function PhoneShell({
	children,
	bottom,
	onSearch,
}: {
	children?: ReactNode;
	bottom?: ReactNode;
	onSearch?: () => void;
}) {
	const queueButtonRef = useRef<HTMLButtonElement>(null);
	const [queueOpen, setQueueOpen] = useState(false);
	const closeQueue = useCallback(() => {
		setQueueOpen(false);
		queueButtonRef.current?.focus();
	}, []);
	return (
		<div
			data-app-shell
			data-phone-shell
			className="relative flex h-full min-h-0 w-full min-w-0 flex-1 flex-col overflow-hidden bg-background text-foreground"
		>
			<header
				data-shell-surface
				className="box-content flex h-12 shrink-0 items-center justify-between gap-2 bg-sidebar px-(--shell-inset,2rem) pt-safe-top text-sidebar-foreground"
			>
				<Link
					to="/"
					aria-label="Harmony"
					title="Harmony"
					className="flex size-11 shrink-0 items-center text-shell-brand"
				>
					<span aria-hidden className="brand-mark size-6 shrink-0" />
				</Link>
				<div className="-mr-3 flex min-w-0 items-center gap-1">
					<TopNavActionsSlot />
					<button
						ref={queueButtonRef}
						type="button"
						aria-label="Open queue"
						title="Open queue"
						aria-haspopup="dialog"
						aria-expanded={queueOpen}
						className={ICON_BUTTON_CLASS}
						onClick={() => setQueueOpen(true)}
					>
						<ListMusic className="size-5" />
					</button>
					{onSearch ? (
						<button
							type="button"
							aria-label="Search your library"
							title="Search your library"
							className={ICON_BUTTON_CLASS}
							onClick={onSearch}
						>
							<Search className="size-5" />
						</button>
					) : null}
				</div>
			</header>
			<main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto bg-background">
				{children}
			</main>
			<div className="shrink-0">{bottom}</div>
			<PhoneTabBar />
			<Toaster />
			{queueOpen ? <PhoneQueueView onClose={closeQueue} /> : null}
		</div>
	);
}

/** The Queue, reachable with nothing loaded, where Now Playing has no Track. */
function PhoneQueueView({ onClose }: { onClose: () => void }) {
	const rootRef = useRef<HTMLDivElement>(null);
	useFocusTrap(rootRef);
	useCloseOnNavigate(onClose);
	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);
	return createPortal(
		<div
			ref={rootRef}
			role="dialog"
			aria-modal="true"
			aria-label="Queue"
			tabIndex={-1}
			data-now-playing-surface
			className="motion-safe:animate-lyrics-view fixed inset-0 z-fullscreen flex flex-col overflow-hidden bg-background pt-safe-top pb-safe-bottom text-queue-foreground outline-none"
		>
			<QueuePanel embedded onClose={onClose} />
		</div>,
		document.body,
	);
}

export function PhoneTabBar() {
	const listenTogether = useContext(ListenTogetherContext);
	const role = listenTogether?.role;
	const isRoomOpen = role === "owner" && Boolean(listenTogether?.room);
	const { pathname } = useLocation();
	const moreButtonRef = useRef<HTMLButtonElement>(null);
	const [moreOpen, setMoreOpen] = useState(false);
	const closeMore = useCallback(() => {
		setMoreOpen(false);
		// iOS does not focus a tapped button, so the trap has nothing to restore.
		moreButtonRef.current?.focus();
	}, []);
	const { tabs, more } = phoneNav(role);
	const isOnMoreRoute = more.some(
		({ to }) => pathname === to || pathname.startsWith(`${to}/`),
	);
	return (
		<nav
			aria-label="Library"
			data-shell-surface
			className="box-content flex h-14 shrink-0 items-center gap-1 border-sidebar-border border-t bg-sidebar px-(--shell-inset,2rem) pb-safe-bottom text-sidebar-foreground"
		>
			{tabs.map(({ to, label, icon: Icon }) => (
				<Link key={to} to={to} className={TAB_CLASS}>
					<Icon className="size-5 shrink-0" />
					<span className="max-w-full truncate">{label}</span>
				</Link>
			))}
			<button
				ref={moreButtonRef}
				type="button"
				aria-haspopup="dialog"
				aria-expanded={moreOpen}
				className={cn(TAB_CLASS, (moreOpen || isOnMoreRoute) && "active")}
				onClick={() => setMoreOpen(true)}
			>
				<Ellipsis className="size-5 shrink-0" />
				More
			</button>
			{moreOpen ? (
				<MoreSheet links={more} isRoomOpen={isRoomOpen} onClose={closeMore} />
			) : null}
		</nav>
	);
}

function MoreSheet({
	links,
	isRoomOpen,
	onClose,
}: {
	links: ReturnType<typeof phoneNav>["more"];
	isRoomOpen: boolean;
	onClose: () => void;
}) {
	const sheetRef = useRef<HTMLDivElement>(null);
	useFocusTrap(sheetRef);
	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);
	return createPortal(
		<div className="fixed inset-0 z-50 flex flex-col bg-background/70">
			{/* Tapping outside closes; the sheet's own button serves keyboards. */}
			<button
				type="button"
				tabIndex={-1}
				aria-label="Close more"
				className="min-h-0 flex-1"
				onClick={onClose}
			/>
			<div
				ref={sheetRef}
				role="dialog"
				aria-modal="true"
				aria-label="More"
				tabIndex={-1}
				className="rounded-t-lg border-border border-t bg-popover px-(--shell-inset,2rem) pt-3 pb-safe-bottom text-popover-foreground shadow-xl outline-none"
			>
				<div className="flex items-center justify-between">
					<h2 className="font-semibold text-heading text-base">More</h2>
					<button
						type="button"
						aria-label="Close more"
						title="Close more"
						className={cn(ICON_BUTTON_CLASS, "-mr-3 text-popover-foreground")}
						onClick={onClose}
					>
						<X className="size-5" />
					</button>
				</div>
				<div className="-mx-3 flex flex-col gap-1 pb-3">
					{links.map(({ to, label, icon: Icon }) => {
						const marksRoom = isRoomOpen && to === "/listen-together";
						return (
							<Link
								key={to}
								to={to}
								title={marksRoom ? `${label}: Room open` : label}
								className={cn(
									NAV_LINK_CLASS,
									"relative h-11 text-popover-foreground",
								)}
								onClick={onClose}
							>
								<Icon className="size-5 shrink-0" />
								<span>{label}</span>
								{marksRoom ? <RoomOpenMark /> : null}
							</Link>
						);
					})}
				</div>
			</div>
		</div>,
		document.body,
	);
}
