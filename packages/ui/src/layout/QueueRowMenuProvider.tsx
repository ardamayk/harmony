import type { QueueItem } from "@repo/api-client";
import {
	type ComponentType,
	createContext,
	type MouseEvent,
	type ReactElement,
	type ReactNode,
	useCallback,
	useContext,
} from "react";

export type QueueRowMenuProps = {
	item: QueueItem;
	children: ReactElement;
};

const QueueNavigationContext = createContext<
	((href: string) => void) | undefined
>(undefined);

type RenderQueueRowMenu = (props: QueueRowMenuProps) => ReactElement;

const QueueRowMenuContext = createContext<RenderQueueRowMenu | null>(null);

/** Lets the host reuse its context menus without coupling shared UI to the app. */
export function QueueRowMenuProvider({
	menu: Menu,
	children,
	onNavigate,
}: {
	menu: ComponentType<QueueRowMenuProps>;
	children: ReactNode;
	onNavigate?: (href: string) => void;
}) {
	const renderMenu = useCallback(
		(props: QueueRowMenuProps) => <Menu {...props} />,
		[Menu],
	);
	return (
		<QueueNavigationContext.Provider value={onNavigate}>
			<QueueRowMenuContext.Provider value={renderMenu}>
				{children}
			</QueueRowMenuContext.Provider>
		</QueueNavigationContext.Provider>
	);
}

export function useQueueRowMenu() {
	return useContext(QueueRowMenuContext);
}

export function useQueueNavigation() {
	return useContext(QueueNavigationContext);
}

/** An Artist's page, where Queue rows and headers link a credited Artist. */
export function queueArtistHref(artistId: string): string {
	return `/library/artists/${encodeURIComponent(artistId)}`;
}

/**
 * Click handler for a link inside the Queue: a plain click navigates in the
 * app (a modified one opens the browser's way) and never reaches the row
 * under it, which would play the Track.
 */
export function useQueueLinkClick() {
	const onNavigate = useQueueNavigation();
	return (href: string) => (event: MouseEvent<HTMLAnchorElement>) => {
		event.stopPropagation();
		if (
			onNavigate &&
			event.button === 0 &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.shiftKey &&
			!event.altKey
		) {
			event.preventDefault();
			onNavigate(href);
		}
	};
}
