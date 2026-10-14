import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	describePlaybackShortcuts,
	type PlaybackKeyboardActions,
	usePlaybackKeyboardShortcuts,
} from "./use-playback-keyboard-shortcuts";

function Harness(actions: PlaybackKeyboardActions) {
	usePlaybackKeyboardShortcuts(actions);
	return (
		<div>
			<input aria-label="Search" type="text" />
			<button type="button">Play</button>
			<p>Body</p>
		</div>
	);
}

function renderHarness() {
	const actions = {
		togglePlay: vi.fn(),
		navigatePrevious: vi.fn(),
		navigateNext: vi.fn(),
		seekBy: vi.fn(),
		adjustVolume: vi.fn(),
		toggleMute: vi.fn(),
		toggleLyrics: vi.fn(),
		toggleQueue: vi.fn(),
		toggleHelp: vi.fn(),
	};
	const result = render(<Harness {...actions} />);
	return { ...result, ...actions };
}

describe("usePlaybackKeyboardShortcuts", () => {
	afterEach(cleanup);

	it("toggles playback on Space when nothing editable is focused", () => {
		const { togglePlay } = renderHarness();

		const event = new KeyboardEvent("keydown", {
			key: " ",
			bubbles: true,
			cancelable: true,
		});
		document.body.dispatchEvent(event);

		expect(togglePlay).toHaveBeenCalledTimes(1);
		expect(event.defaultPrevented).toBe(true);
	});

	it("leaves keys alone inside text inputs and on buttons", () => {
		const { togglePlay, navigateNext, getByLabelText, getByRole } =
			renderHarness();

		fireEvent.keyDown(getByLabelText("Search"), { key: " " });
		fireEvent.keyDown(getByLabelText("Search"), { key: "n" });
		fireEvent.keyDown(getByRole("button", { name: "Play" }), { key: " " });

		expect(togglePlay).not.toHaveBeenCalled();
		expect(navigateNext).not.toHaveBeenCalled();
	});

	it("ignores held-down key repeats and modified keys", () => {
		const { togglePlay, navigateNext } = renderHarness();

		fireEvent.keyDown(document.body, { key: " ", repeat: true });
		fireEvent.keyDown(document.body, { key: " ", ctrlKey: true });
		fireEvent.keyDown(document.body, { key: "n", metaKey: true });
		fireEvent.keyDown(document.body, { key: "n", repeat: true });

		expect(togglePlay).not.toHaveBeenCalled();
		expect(navigateNext).not.toHaveBeenCalled();
	});

	it("maps media keys to playback actions", () => {
		const { togglePlay, navigateNext, navigatePrevious } = renderHarness();

		fireEvent.keyDown(document.body, { key: "MediaPlayPause" });
		fireEvent.keyDown(document.body, { key: "MediaTrackNext" });
		fireEvent.keyDown(document.body, { key: "MediaTrackPrevious" });

		expect(togglePlay).toHaveBeenCalledTimes(1);
		expect(navigateNext).toHaveBeenCalledTimes(1);
		expect(navigatePrevious).toHaveBeenCalledTimes(1);
	});

	it("seeks with the arrow keys and further with Shift", () => {
		const { seekBy } = renderHarness();

		fireEvent.keyDown(document.body, { key: "ArrowRight" });
		fireEvent.keyDown(document.body, { key: "ArrowLeft" });
		fireEvent.keyDown(document.body, { key: "ArrowRight", shiftKey: true });
		fireEvent.keyDown(document.body, { key: "ArrowLeft", shiftKey: true });

		expect(seekBy.mock.calls.map(([delta]) => delta)).toEqual([5, -5, 30, -30]);
	});

	it("changes volume with the vertical arrows and mutes with M", () => {
		const { adjustVolume, toggleMute } = renderHarness();

		fireEvent.keyDown(document.body, { key: "ArrowUp" });
		fireEvent.keyDown(document.body, { key: "ArrowDown" });
		fireEvent.keyDown(document.body, { key: "M" });

		expect(adjustVolume.mock.calls.map(([delta]) => delta)).toEqual([
			0.05, -0.05,
		]);
		expect(toggleMute).toHaveBeenCalledTimes(1);
	});

	it("maps letters to navigation, lyrics, queue and help", () => {
		const {
			navigateNext,
			navigatePrevious,
			toggleLyrics,
			toggleQueue,
			toggleHelp,
		} = renderHarness();

		fireEvent.keyDown(document.body, { key: "n" });
		fireEvent.keyDown(document.body, { key: "p" });
		fireEvent.keyDown(document.body, { key: "l" });
		fireEvent.keyDown(document.body, { key: "q" });
		fireEvent.keyDown(document.body, { key: "?", shiftKey: true });

		expect(navigateNext).toHaveBeenCalledTimes(1);
		expect(navigatePrevious).toHaveBeenCalledTimes(1);
		expect(toggleLyrics).toHaveBeenCalledTimes(1);
		expect(toggleQueue).toHaveBeenCalledTimes(1);
		expect(toggleHelp).toHaveBeenCalledTimes(1);
	});

	it("does not claim keys whose action is not provided", () => {
		const togglePlay = vi.fn();
		render(
			<Harness
				togglePlay={togglePlay}
				navigatePrevious={vi.fn()}
				navigateNext={vi.fn()}
			/>,
		);
		const event = new KeyboardEvent("keydown", {
			key: "ArrowRight",
			bubbles: true,
			cancelable: true,
		});
		document.body.dispatchEvent(event);
		expect(event.defaultPrevented).toBe(false);
	});

	it("stops listening after unmount", () => {
		const { togglePlay, unmount } = renderHarness();
		unmount();

		fireEvent.keyDown(document.body, { key: " " });

		expect(togglePlay).not.toHaveBeenCalled();
	});

	it("describes every binding for the help overlay", () => {
		const descriptions = describePlaybackShortcuts({
			seekStepSeconds: 10,
			seekStepLargeSeconds: 60,
		});
		expect(descriptions.map((entry) => entry.keys.join("+"))).toContain(
			"Space",
		);
		expect(
			descriptions.find((entry) => entry.keys.join("") === "←→")?.description,
		).toBe("Seek 10 seconds");
		expect(
			descriptions.find((entry) => entry.keys.includes("Shift"))?.description,
		).toBe("Seek 60 seconds");
	});
});
