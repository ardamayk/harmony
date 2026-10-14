import { defaultPreferences, LayoutProvider } from "@repo/ui";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { PlaybackSettingsSection } from "./-playback-settings";

describe("PlaybackSettingsSection", () => {
	afterEach(cleanup);

	it("persists choices and toggles through LayoutProvider", () => {
		const onPreferencesChange = vi.fn();
		render(
			<LayoutProvider
				initialPreferences={defaultPreferences}
				onPreferencesChange={onPreferencesChange}
			>
				<PlaybackSettingsSection />
			</LayoutProvider>,
		);

		expect(
			screen.getByRole("button", { name: "1×" }).getAttribute("aria-pressed"),
		).toBe("true");
		fireEvent.click(screen.getByRole("button", { name: "1.5×" }));
		expect(
			screen.getByRole("button", { name: "1.5×" }).getAttribute("aria-pressed"),
		).toBe("true");
		expect(onPreferencesChange).toHaveBeenLastCalledWith(
			expect.objectContaining({
				playback: expect.objectContaining({ playbackRate: 1.5 }),
			}),
		);

		fireEvent.click(screen.getByRole("button", { name: "Wait for me" }));
		expect(onPreferencesChange).toHaveBeenLastCalledWith(
			expect.objectContaining({
				playback: expect.objectContaining({
					playbackRate: 1.5,
					autoSkipOnErrorSeconds: 0,
				}),
			}),
		);

		const waveform = screen.getByRole("switch", { name: "Waveform seek bar" });
		expect(waveform.getAttribute("aria-checked")).toBe("true");
		fireEvent.click(waveform);
		expect(onPreferencesChange).toHaveBeenLastCalledWith(
			expect.objectContaining({
				playback: expect.objectContaining({ showWaveform: false }),
			}),
		);
	});
});
