import type { QueueItem } from "@repo/api-client";
import {
	type ComponentType,
	createContext,
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
