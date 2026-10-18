import type {
	LayoutPreferences,
	PlaybackPreferences,
	UserPreferences,
} from "@repo/api-client";
import {
	createContext,
	type ReactNode,
	useCallback,
	useContext,
	useMemo,
	useRef,
	useState,
} from "react";
import {
	clampPanelSizes,
	MINI_PANEL_SIZE,
	normalizeLayout,
} from "../widgets/layout-utils";
import { defaultPlayback, defaultPreferences } from "../widgets/types";

type PanelSide = "left" | "right";

type LayoutContextValue = {
	preferences: UserPreferences;
	setPreferences: (next: Partial<UserPreferences>) => void;
	togglePanel: (side: PanelSide) => void;
	moveSidebar: (position: "left" | "right") => void;
	setPanelSizes: (sizes: [number, number, number]) => void;
	reorderWidgets: (panel: PanelSide, activeId: string, overId: string) => void;
	moveWidgetToPanel: (
		widgetId: string,
		targetPanel: PanelSide,
		index: number,
	) => void;
	resetLayout: () => void;
};

const LayoutContext = createContext<LayoutContextValue | null>(null);

export function LayoutProvider({
	children,
	initialPreferences = defaultPreferences,
	onPreferencesChange,
}: {
	children: ReactNode;
	initialPreferences?: UserPreferences;
	onPreferencesChange?: (prefs: UserPreferences) => void;
}) {
	const [preferences, setPreferencesState] = useState<UserPreferences>({
		...initialPreferences,
		layout: normalizeLayout(initialPreferences.layout),
		// Older servers answer without a playback section; fill it in so the
		// Player Bar never reads an undefined preference.
		playback: { ...defaultPlayback, ...initialPreferences.playback },
	});
	const expandedSizesRef = useRef({ left: 22, right: 28 });

	const commit = useCallback(
		(next: UserPreferences) => {
			const normalized = {
				...next,
				layout: normalizeLayout(next.layout),
			};
			setPreferencesState(normalized);
			onPreferencesChange?.(normalized);
		},
		[onPreferencesChange],
	);

	const setPreferences = useCallback(
		(patch: Partial<UserPreferences>) => {
			commit({
				...preferences,
				...patch,
				theme: patch.theme
					? { ...preferences.theme, ...patch.theme }
					: preferences.theme,
				layout: patch.layout
					? normalizeLayout({ ...preferences.layout, ...patch.layout })
					: preferences.layout,
				playback: patch.playback
					? { ...preferences.playback, ...patch.playback }
					: preferences.playback,
			});
		},
		[commit, preferences],
	);

	const togglePanel = useCallback(
		(side: PanelSide) => {
			const collapsed = { ...preferences.layout.collapsed };
			const sizes = [...(preferences.layout.sizes ?? [22, 50, 28])] as [
				number,
				number,
				number,
			];
			const panelIndex = side === "left" ? 0 : 2;

			if (!collapsed[side]) {
				expandedSizesRef.current[side] = sizes[panelIndex];
				const delta = sizes[panelIndex] - MINI_PANEL_SIZE;
				sizes[panelIndex] = MINI_PANEL_SIZE;
				sizes[1] += delta;
				collapsed[side] = true;
			} else {
				const restore = expandedSizesRef.current[side];
				const delta = restore - sizes[panelIndex];
				sizes[panelIndex] = restore;
				sizes[1] -= delta;
				collapsed[side] = false;
			}

			const layout: LayoutPreferences = {
				...preferences.layout,
				collapsed,
				sizes: clampPanelSizes(sizes, collapsed),
			};
			commit({ ...preferences, layout });
		},
		[commit, preferences],
	);

	const moveSidebar = useCallback(
		(position: "left" | "right") => {
			commit({
				...preferences,
				layout: { ...preferences.layout, sidebarPosition: position },
			});
		},
		[commit, preferences],
	);

	const setPanelSizes = useCallback(
		(sizes: [number, number, number]) => {
			commit({
				...preferences,
				layout: { ...preferences.layout, sizes },
			});
		},
		[commit, preferences],
	);

	const reorderWidgets = useCallback(
		(panel: PanelSide, activeId: string, overId: string) => {
			const ids = [...preferences.layout.panels[panel]];
			const oldIndex = ids.indexOf(activeId);
			const newIndex = ids.indexOf(overId);
			if (oldIndex < 0 || newIndex < 0 || oldIndex === newIndex) return;
			ids.splice(oldIndex, 1);
			ids.splice(newIndex, 0, activeId);
			commit({
				...preferences,
				layout: {
					...preferences.layout,
					panels: { ...preferences.layout.panels, [panel]: ids },
				},
			});
		},
		[commit, preferences],
	);

	const moveWidgetToPanel = useCallback(
		(widgetId: string, targetPanel: PanelSide, index: number) => {
			const left = preferences.layout.panels.left.filter(
				(id) => id !== widgetId,
			);
			const right = preferences.layout.panels.right.filter(
				(id) => id !== widgetId,
			);
			const target = targetPanel === "left" ? [...left] : [...right];
			target.splice(index, 0, widgetId);
			commit({
				...preferences,
				layout: {
					...preferences.layout,
					panels: {
						left: targetPanel === "left" ? target : left,
						right: targetPanel === "right" ? target : right,
					},
				},
			});
		},
		[commit, preferences],
	);

	const resetLayout = useCallback(() => {
		commit(defaultPreferences);
	}, [commit]);

	const value = useMemo(
		() => ({
			preferences,
			setPreferences,
			togglePanel,
			moveSidebar,
			setPanelSizes,
			reorderWidgets,
			moveWidgetToPanel,
			resetLayout,
		}),
		[
			preferences,
			setPreferences,
			togglePanel,
			moveSidebar,
			setPanelSizes,
			reorderWidgets,
			moveWidgetToPanel,
			resetLayout,
		],
	);

	return (
		<LayoutContext.Provider value={value}>{children}</LayoutContext.Provider>
	);
}

export function useLayout() {
	const ctx = useContext(LayoutContext);
	if (!ctx) {
		throw new Error("useLayout must be used within LayoutProvider");
	}
	return ctx;
}

/** Playback Preferences plus a sparse setter; convenience over useLayout. */
export function usePlaybackPreferences() {
	const { preferences, setPreferences } = useLayout();
	const setPlaybackPreferences = useCallback(
		(patch: Partial<PlaybackPreferences>) =>
			setPreferences({ playback: { ...preferences.playback, ...patch } }),
		[preferences.playback, setPreferences],
	);
	return { playback: preferences.playback, setPlaybackPreferences };
}
