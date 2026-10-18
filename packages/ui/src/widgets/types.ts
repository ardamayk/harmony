import type {
	LayoutPreferences,
	PlaybackPreferences,
	ThemePreferences,
	UserPreferences,
} from "@repo/api-client";
import type { ComponentType } from "react";

export type WidgetDefinition = {
	id: string;
	title: string;
	component: ComponentType;
};

export type WidgetPlacement = "left" | "right" | "main";

export const defaultLayout: LayoutPreferences = {
	sidebarPosition: "left",
	panels: {
		left: ["now-playing"],
		right: ["discover"],
	},
	collapsed: { left: false, right: false },
	sizes: [22, 50, 28],
};

export const defaultTheme: ThemePreferences = {
	mode: "system",
	preset: "earthly",
};

export const defaultPlayback: PlaybackPreferences = {
	seekStepSeconds: 5,
	seekStepLargeSeconds: 30,
	playbackRate: 1,
	transitionFadeMs: 0,
	showWaveform: true,
	accentFromCover: false,
	showUpNext: true,
	autoSkipOnErrorSeconds: 5,
	hoverTimestamp: true,
};

export const defaultPreferences: UserPreferences = {
	theme: defaultTheme,
	layout: defaultLayout,
	playback: defaultPlayback,
};
