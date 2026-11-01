import type { UserPreferences } from "@repo/api-client";
import { cleanup, render, screen } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { defaultPreferences } from "../widgets/types";
import { LayoutProvider, useLayout } from "./LayoutProvider";

const defaultPosition = defaultPreferences.layout.sidebarPosition;
const otherPosition = defaultPosition === "left" ? "right" : "left";

function preferencesAt(sidebarPosition: "left" | "right"): UserPreferences {
	return {
		...defaultPreferences,
		layout: { ...defaultPreferences.layout, sidebarPosition },
	};
}

function Probe({ onMount }: { onMount: () => void }) {
	const { preferences } = useLayout();
	useEffect(onMount, [onMount]);
	return <p>{preferences.layout.sidebarPosition}</p>;
}

describe("LayoutProvider", () => {
	afterEach(cleanup);

	it("takes preferences that arrive late once, in place and unsaved", () => {
		const onMount = vi.fn();
		const onPreferencesChange = vi.fn();
		const tree = (loadedPreferences?: UserPreferences) => (
			<LayoutProvider
				loadedPreferences={loadedPreferences}
				onPreferencesChange={onPreferencesChange}
			>
				<Probe onMount={onMount} />
			</LayoutProvider>
		);
		const { rerender } = render(tree());
		expect(screen.getByText(defaultPosition)).toBeTruthy();

		rerender(tree(preferencesAt(otherPosition)));
		expect(screen.getByText(otherPosition)).toBeTruthy();

		// What the Music Server answers to a save is not news to the provider.
		rerender(tree(preferencesAt(defaultPosition)));
		expect(screen.getByText(otherPosition)).toBeTruthy();

		expect(onMount).toHaveBeenCalledOnce();
		expect(onPreferencesChange).not.toHaveBeenCalled();
	});

	it("does not replace preferences it opened with", () => {
		const tree = (loadedPreferences: UserPreferences) => (
			<LayoutProvider
				initialPreferences={preferencesAt(otherPosition)}
				loadedPreferences={loadedPreferences}
			>
				<Probe onMount={() => undefined} />
			</LayoutProvider>
		);
		const { rerender } = render(tree(preferencesAt(otherPosition)));

		rerender(tree(preferencesAt(defaultPosition)));

		expect(screen.getByText(otherPosition)).toBeTruthy();
	});
});
