import type { Album } from "@repo/api-client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { AlbumShelf } from "./album-shelf";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		className,
	}: {
		children: React.ReactNode;
		className?: string;
	}) => (
		<a href="/album" className={className}>
			{children}
		</a>
	),
}));

vi.mock("#/lib/api", () => ({
	apiClient: { getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}` },
}));

const albums = ["One", "Two", "Three"].map(
	(title, index) =>
		({
			id: `al${index}`,
			title,
			artistId: "ar",
			artistName: "Artist",
		}) as Album,
);

/** The row's scroll geometry, which jsdom does not lay out. */
function setRow(row: HTMLElement, scrollLeft: number) {
	Object.defineProperty(row, "clientWidth", { value: 300, configurable: true });
	Object.defineProperty(row, "scrollWidth", { value: 700, configurable: true });
	Object.defineProperty(row, "scrollLeft", {
		value: scrollLeft,
		configurable: true,
	});
	fireEvent.scroll(row);
}

describe("AlbumShelf", () => {
	afterEach(cleanup);

	it("disables an arrow when the row cannot scroll that way", () => {
		const { container } = render(
			<AlbumShelf heading={<h2>Discography</h2>} albums={albums} />,
		);
		const left = screen.getByRole("button", { name: "Scroll left" });
		const right = screen.getByRole("button", { name: "Scroll right" });
		// Nothing overflows before layout: both ends at once.
		expect([
			left.hasAttribute("disabled"),
			right.hasAttribute("disabled"),
		]).toEqual([true, true]);
		const row = container.querySelector(".overflow-x-auto") as HTMLElement;
		setRow(row, 0);
		expect([
			left.hasAttribute("disabled"),
			right.hasAttribute("disabled"),
		]).toEqual([true, false]);
		setRow(row, 200);
		expect([
			left.hasAttribute("disabled"),
			right.hasAttribute("disabled"),
		]).toEqual([false, false]);
		setRow(row, 400);
		expect([
			left.hasAttribute("disabled"),
			right.hasAttribute("disabled"),
		]).toEqual([false, true]);
	});
});
