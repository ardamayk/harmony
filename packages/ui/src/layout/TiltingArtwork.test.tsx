import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { TiltingArtwork } from "./TiltingArtwork";

function renderArtwork(hasReducedMotion = false) {
	vi.stubGlobal(
		"matchMedia",
		vi.fn(() => ({ matches: hasReducedMotion })),
	);
	const result = render(
		<TiltingArtwork coverUrl="/cover/album-1" title="Album 1" />,
	);
	const artwork = screen.getByTestId("tilting-artwork");
	const surface = result.container.firstElementChild as HTMLElement;
	vi.spyOn(surface, "getBoundingClientRect").mockReturnValue(
		new DOMRect(100, 100, 200, 200),
	);
	return { artwork, surface };
}

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
});

describe("TiltingArtwork", () => {
	it("tilts in opposite directions across the artwork and stays flat at its center", () => {
		const { artwork, surface } = renderArtwork();
		fireEvent.pointerMove(surface, {
			pointerType: "mouse",
			clientX: 100,
			clientY: 200,
		});
		expect(artwork.style.transform).toBe("rotateX(0deg) rotateY(-12deg)");
		fireEvent.pointerMove(surface, {
			pointerType: "mouse",
			clientX: 300,
			clientY: 200,
		});
		expect(artwork.style.transform).toBe("rotateX(0deg) rotateY(12deg)");
		fireEvent.pointerMove(surface, {
			pointerType: "mouse",
			clientX: 200,
			clientY: 200,
		});
		expect(artwork.style.transform).toBe("rotateX(0deg) rotateY(0deg)");
	});

	it.each(["pointerLeave", "pointerCancel"] as const)(
		"resets perspective on %s",
		(event) => {
			const { artwork, surface } = renderArtwork();
			fireEvent.pointerMove(surface, {
				pointerType: "mouse",
				clientX: 300,
				clientY: 100,
			});
			expect(artwork.style.transform).toBe("rotateX(12deg) rotateY(12deg)");
			fireEvent[event](surface);
			expect(artwork.style.transform).toBe("rotateX(0deg) rotateY(0deg)");
		},
	);

	it.each([
		{ name: "reduced motion", hasReducedMotion: true, pointerType: "mouse" },
		{ name: "touch input", hasReducedMotion: false, pointerType: "touch" },
	])("does not tilt for $name", ({ hasReducedMotion, pointerType }) => {
		const { artwork, surface } = renderArtwork(hasReducedMotion);
		fireEvent.pointerMove(surface, { pointerType, clientX: 300, clientY: 100 });
		expect(artwork.style.transform).toBe("");
	});
});
