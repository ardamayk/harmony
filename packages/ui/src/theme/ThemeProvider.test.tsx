import { render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { ThemeProvider } from "./ThemeProvider";

describe("ThemeProvider", () => {
	afterEach(() => {
		document.documentElement.classList.remove("dark");
		delete document.documentElement.dataset.themePreset;
	});

	it("sets data-theme-preset on the document root", () => {
		render(
			<ThemeProvider theme={{ mode: "dark", preset: "vintage-harbor" }} />,
		);
		expect(document.documentElement.dataset.themePreset).toBe("vintage-harbor");
	});

	it("toggles the dark class from theme mode", () => {
		const { rerender } = render(
			<ThemeProvider theme={{ mode: "light", preset: "earthly" }} />,
		);
		expect(document.documentElement.classList.contains("dark")).toBe(false);

		rerender(<ThemeProvider theme={{ mode: "dark", preset: "earthly" }} />);
		expect(document.documentElement.classList.contains("dark")).toBe(true);
	});
});
