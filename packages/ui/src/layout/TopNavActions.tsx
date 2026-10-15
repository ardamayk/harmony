import {
	createContext,
	type ReactNode,
	useContext,
	useMemo,
	useState,
} from "react";
import { createPortal } from "react-dom";

type TopNavActionsSlot = {
	slot: HTMLElement | null;
	setSlot: (element: HTMLElement | null) => void;
};

const TopNavActionsContext = createContext<TopNavActionsSlot | null>(null);

/** Lets a page hand controls to the Top Nav, next to Settings. */
export function TopNavActionsProvider({ children }: { children: ReactNode }) {
	const [slot, setSlot] = useState<HTMLElement | null>(null);
	const value = useMemo(() => ({ slot, setSlot }), [slot]);
	return (
		<TopNavActionsContext.Provider value={value}>
			{children}
		</TopNavActionsContext.Provider>
	);
}

/** Where the Top Nav renders page actions; empty when no page supplies any. */
export function TopNavActionsSlot() {
	const setSlot = useContext(TopNavActionsContext)?.setSlot;
	if (!setSlot) return null;
	return (
		<div
			ref={setSlot}
			data-top-nav-actions
			className="flex shrink-0 items-center gap-1 empty:hidden"
		/>
	);
}

/**
 * Renders a page's controls in the Top Nav. Outside an App Shell (tests,
 * isolated hosts) there is no Top Nav, so the controls render in place.
 */
export function TopNavActions({ children }: { children: ReactNode }) {
	const context = useContext(TopNavActionsContext);
	if (!context) return <>{children}</>;
	if (!context.slot) return null;
	return createPortal(children, context.slot);
}
