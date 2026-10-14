import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { formatSeekTime, fractionForPointer, SeekBar } from "./SeekBar";

function stubRect(element: HTMLElement, left = 100, width = 200) {
	element.getBoundingClientRect = () =>
		({
			left,
			width,
			top: 0,
			height: 24,
			right: left + width,
			bottom: 24,
			x: left,
			y: 0,
			toJSON: () => undefined,
		}) as DOMRect;
	element.setPointerCapture = vi.fn();
	element.releasePointerCapture = vi.fn();
}

describe("SeekBar", () => {
	afterEach(cleanup);

	it("clamps pointer fractions to the bar", () => {
		const rect = { left: 100, width: 200 };
		expect(fractionForPointer(50, rect)).toBe(0);
		expect(fractionForPointer(150, rect)).toBe(0.25);
		expect(fractionForPointer(500, rect)).toBe(1);
		expect(fractionForPointer(150, { left: 0, width: 0 })).toBe(0);
	});

	it("formats seconds as m:ss", () => {
		expect(formatSeekTime(0)).toBe("0:00");
		expect(formatSeekTime(65.9)).toBe("1:05");
		expect(formatSeekTime(Number.NaN)).toBe("0:00");
	});

	it("shows the hovered timestamp and a buffered layer", () => {
		render(
			<SeekBar
				currentTime={30}
				duration={200}
				bufferedEnd={100}
				onSeek={() => undefined}
			/>,
		);
		const bar = screen.getByTestId("seek-bar");
		stubRect(bar);

		fireEvent.pointerMove(bar, { clientX: 200, pointerType: "mouse" });
		expect(screen.getByTestId("seek-tooltip").textContent).toBe("1:40");

		const buffered = screen.getByTestId("seek-buffered")
			.firstElementChild as HTMLElement;
		expect(buffered.style.width).toBe("50%");

		fireEvent.pointerLeave(bar);
		expect(screen.queryByTestId("seek-tooltip")).toBeNull();
	});

	it("scrubs with pointer capture, clamps past the end and seeks on release", () => {
		const onSeek = vi.fn();
		render(<SeekBar currentTime={0} duration={100} onSeek={onSeek} />);
		const bar = screen.getByTestId("seek-bar");
		stubRect(bar);

		fireEvent.pointerDown(bar, { clientX: 150, pointerId: 1, button: 0 });
		expect(bar.setPointerCapture).toHaveBeenCalledWith(1);
		expect(bar.dataset.scrubbing).toBe("true");
		expect(
			(
				screen.getByLabelText("Seek") as HTMLInputElement
			).style.getPropertyValue("--seek-level"),
		).toBe("25.00%");

		fireEvent.pointerMove(bar, { clientX: 900, pointerId: 1 });
		expect(
			(
				screen.getByLabelText("Seek") as HTMLInputElement
			).style.getPropertyValue("--seek-level"),
		).toBe("100.00%");
		expect(onSeek).not.toHaveBeenCalled();

		fireEvent.pointerUp(bar, { clientX: 250, pointerId: 1 });
		expect(onSeek).toHaveBeenCalledWith(75);
		expect(bar.dataset.scrubbing).toBeUndefined();
	});

	it("cancels a scrub on Escape without seeking", () => {
		const onSeek = vi.fn();
		render(<SeekBar currentTime={10} duration={100} onSeek={onSeek} />);
		const bar = screen.getByTestId("seek-bar");
		stubRect(bar);

		fireEvent.pointerDown(bar, { clientX: 200, pointerId: 2, button: 0 });
		fireEvent.keyDown(document, { key: "Escape" });
		expect(bar.dataset.scrubbing).toBeUndefined();

		fireEvent.pointerUp(bar, { clientX: 200, pointerId: 2 });
		expect(onSeek).not.toHaveBeenCalled();
	});

	it("draws waveform bars behind the track and clips the played part", () => {
		render(
			<SeekBar
				currentTime={25}
				duration={100}
				waveform={[0, 128, 255, 64]}
				onSeek={() => undefined}
			/>,
		);
		const bar = screen.getByTestId("seek-bar");
		expect(bar.dataset.waveform).toBe("true");
		const svg = screen.getByTestId("seek-waveform");
		expect(svg.querySelectorAll("rect")).toHaveLength(12);
		const played = screen.getByTestId("seek-waveform-played") as HTMLElement;
		expect(played.style.clipPath).toBe("inset(0 75.00% 0 0)");
		expect(screen.getByLabelText("Seek").className).toContain(
			"player-seek-slider--waveform",
		);
	});

	it("moves by the configured steps from the arrow keys", () => {
		const onSeek = vi.fn();
		render(
			<SeekBar
				currentTime={40}
				duration={100}
				keyboardStepSeconds={10}
				keyboardLargeStepSeconds={70}
				onSeek={onSeek}
			/>,
		);
		// The harness keeps currentTime at 40, so every press starts from 40.
		const input = screen.getByLabelText("Seek");
		fireEvent.keyDown(input, { key: "ArrowRight" });
		expect(onSeek).toHaveBeenLastCalledWith(50);
		fireEvent.keyDown(input, { key: "ArrowLeft" });
		expect(onSeek).toHaveBeenLastCalledWith(30);
		fireEvent.keyDown(input, { key: "ArrowRight", shiftKey: true });
		expect(onSeek).toHaveBeenLastCalledWith(100);
		fireEvent.keyDown(input, { key: "ArrowLeft", shiftKey: true });
		expect(onSeek).toHaveBeenLastCalledWith(0);
	});

	it("keeps the keyboard path on the native range input", () => {
		const onSeek = vi.fn();
		render(<SeekBar currentTime={0} duration={100} onSeek={onSeek} />);
		const input = screen.getByLabelText("Seek") as HTMLInputElement;
		fireEvent.change(input, { target: { value: "0.5" } });
		expect(onSeek).toHaveBeenCalledWith(50);
		expect(input.getAttribute("aria-valuetext")).toBe("0:00 of 1:40");
	});

	it("ignores pointer input while disabled or without a duration", () => {
		const onSeek = vi.fn();
		render(<SeekBar currentTime={0} duration={0} onSeek={onSeek} />);
		const bar = screen.getByTestId("seek-bar");
		stubRect(bar);
		fireEvent.pointerDown(bar, { clientX: 150, pointerId: 3, button: 0 });
		fireEvent.pointerUp(bar, { clientX: 150, pointerId: 3 });
		expect(onSeek).not.toHaveBeenCalled();
		expect(screen.queryByTestId("seek-tooltip")).toBeNull();
	});
});
