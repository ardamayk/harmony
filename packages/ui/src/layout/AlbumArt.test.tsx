import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vite-plus/test";
import { AlbumArt } from "./AlbumArt";

describe("AlbumArt", () => {
	it("uses the first visible title character for fallback artwork", () => {
		render(<AlbumArt title={" \tBlasmusikradio mit Bernd"} coverUrl={null} />);

		expect(screen.getByText("B")).toBeTruthy();
	});
});
