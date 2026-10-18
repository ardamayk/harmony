import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { HomePage } from "./-home-page";

describe("home route", () => {
	afterEach(() => {
		cleanup();
	});

	it("renders without any legacy scan action", () => {
		render(<HomePage />);

		expect(screen.getByRole("heading", { name: "Home" })).toBeTruthy();
		expect(screen.queryByRole("button", { name: /scan/i })).toBeNull();
	});
});
