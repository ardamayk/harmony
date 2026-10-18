import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { AlbumArt } from "./AlbumArt";

describe("AlbumArt", () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it("uses the first visible title character for fallback artwork", () => {
		render(<AlbumArt title={" \tBlasmusikradio mit Bernd"} coverUrl={null} />);

		expect(screen.getByText("B")).toBeTruthy();
	});

	it("asks a failed image once more, then shows the letter", () => {
		vi.useFakeTimers();
		const { container } = render(<AlbumArt title="Awake" coverUrl="/c/1" />);
		fireEvent.error(container.querySelector("img") as HTMLImageElement);
		expect(screen.getByText("A")).toBeTruthy();
		act(() => {
			vi.advanceTimersByTime(2000);
		});
		const retried = container.querySelector("img") as HTMLImageElement;
		expect(retried.getAttribute("src")).toBe("/c/1");
		fireEvent.error(retried);
		act(() => {
			vi.advanceTimersByTime(10_000);
		});
		expect(container.querySelector("img")).toBeNull();
		expect(screen.getByText("A")).toBeTruthy();
	});

	it("tries a new cover even after the last one failed", () => {
		const { container, rerender } = render(
			<AlbumArt title="Awake" coverUrl="/c/1" />,
		);
		fireEvent.error(container.querySelector("img") as HTMLImageElement);
		expect(container.querySelector("img")).toBeNull();
		rerender(<AlbumArt title="Hero" coverUrl="/c/2" />);
		expect(container.querySelector("img")?.getAttribute("src")).toBe("/c/2");
	});
});
