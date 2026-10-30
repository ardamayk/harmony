import {
	createContext,
	type ReactNode,
	useContext,
	useMemo,
	useState,
} from "react";
import { createPortal } from "react-dom";

type TopNavSlots = {
	slot: HTMLElement | null;
	setSlot: (element: HTMLElement | null) => void;
	backdrop: HTMLElement | null;
	setBackdrop: (element: HTMLElement | null) => void;
};

const TopNavSlotsContext = createContext<TopNavSlots | null>(null);

/**
 * Lets a page hand the Top Nav controls, next to Settings, and a backdrop
 * behind the bar.
 */
export function TopNavActionsProvider({ children }: { children: ReactNode }) {
	const [slot, setSlot] = useState<HTMLElement | null>(null);
	const [backdrop, setBackdrop] = useState<HTMLElement | null>(null);
	const value = useMemo(
		() => ({ slot, setSlot, backdrop, setBackdrop }),
		[slot, backdrop],
	);
	return (
		<TopNavSlotsContext.Provider value={value}>
			{children}
		</TopNavSlotsContext.Provider>
	);
}

/** Where the Top Nav renders page actions; empty when no page supplies any. */
export function TopNavActionsSlot() {
	const setSlot = useContext(TopNavSlotsContext)?.setSlot;
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
	const context = useContext(TopNavSlotsContext);
	if (!context) return <>{children}</>;
	if (!context.slot) return null;
	return createPortal(children, context.slot);
}

/**
 * The Top Nav's backdrop layer: the bar to the right of the Sidebar column,
 * clipped to it, behind the bar's content. Clicks and window drags pass
 * through it, and with nothing in it the bar stays flat.
 */
export function TopNavBackdropSlot() {
	const setBackdrop = useContext(TopNavSlotsContext)?.setBackdrop;
	if (!setBackdrop) return null;
	return (
		<div
			ref={setBackdrop}
			aria-hidden
			data-top-nav-backdrop
			className="pointer-events-none absolute inset-y-0 right-0 left-62.5 overflow-hidden max-lg:left-17"
		/>
	);
}

/**
 * Renders a page's backdrop behind the Top Nav. Outside an App Shell there is
 * no bar to paint, so it renders nothing.
 */
export function TopNavBackdrop({ children }: { children: ReactNode }) {
	const backdrop = useContext(TopNavSlotsContext)?.backdrop;
	return backdrop ? createPortal(children, backdrop) : null;
}
