import type { Artist } from "@repo/api-client";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vite-plus/test";
import { ArtistGrid } from "./artist-grid";

describe("ArtistGrid", () => {
	it("uses square cards in the shared collection grid without changing card behavior", () => {
		const artists = [
			{
				id: "artist-1",
				name: "Nina Simone",
				albumCount: 7,
			},
		] as Artist[];

		const { container } = render(<ArtistGrid artists={artists} />);

		const grid = container.firstElementChild;
		expect(grid?.className).toContain(
			"grid-cols-[repeat(auto-fill,min(250px,100%))]",
		);
		expect(grid?.className).toContain("gap-3");

		const card = screen.getByText("Nina Simone").closest("div");
		expect(card?.className).toContain("aspect-square");
		expect(card?.className).toContain("hover:-translate-y-1");
		expect(screen.getByText("N")).toBeTruthy();
		expect(screen.getByText("7 albums")).toBeTruthy();
		expect(screen.queryByRole("link")).toBeNull();
	});
});
