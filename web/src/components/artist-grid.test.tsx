import type { Artist } from "@repo/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";
import { ArtistGrid } from "./artist-grid";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		to,
		params,
		...props
	}: {
		children: React.ReactNode;
		to: string;
		params: { artistId: string };
	}) => (
		<a href={to.replace("$artistId", params.artistId)} {...props}>
			{children}
		</a>
	),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		getArtistPhotoUrl: (artistId: string) => `/photo/${artistId}`,
	},
}));

describe("ArtistGrid", () => {
	it("links square photo cards to the Artist page, with the initial when there is no photo", () => {
		const artists = [
			{ id: "artist-1", name: "Nina Simone", albumCount: 7 },
			{ id: "artist-2", name: "Solo", albumCount: 1 },
		] as Artist[];

		const { container } = render(<ArtistGrid artists={artists} />);

		const grid = container.firstElementChild;
		expect(grid?.className).toContain("grid-cols-cards");
		const card = screen.getByRole("link", { name: /Nina Simone/ });
		expect(card.getAttribute("href")).toBe("/library/artists/artist-1");
		expect(card.className).toContain("aspect-square");
		expect(card.className).toContain("hover:-translate-y-1");
		expect(screen.getByText("7 albums")).toBeTruthy();
		expect(screen.getByText("1 album")).toBeTruthy();

		const photo = card.querySelector("img") as HTMLImageElement;
		expect(photo.getAttribute("src")).toBe("/photo/artist-1");
		expect(photo.getAttribute("loading")).toBe("lazy");
		// The Music Server answers 404 while the Artist has no photo.
		fireEvent.error(photo);
		expect(card.querySelector("img")).toBeNull();
		expect(screen.getByText("N")).toBeTruthy();
	});
});
