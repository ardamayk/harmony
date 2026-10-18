import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { ExternalLinkButton } from "./external-link-button";

const bridge = vi.hoisted(() => ({
	isDesktopClient: vi.fn(() => false),
	openExternalUrl: vi.fn(async () => {}),
}));

vi.mock("#/desktop/bridge", () => bridge);

describe("ExternalLinkButton", () => {
	beforeEach(() => {
		bridge.isDesktopClient.mockReturnValue(false);
		bridge.openExternalUrl.mockClear();
	});
	afterEach(cleanup);

	it("opens a new tab on the web", () => {
		render(
			<ExternalLinkButton
				href="https://open.spotify.com/search/x"
				name="Spotify"
				short="SP"
			/>,
		);
		const link = screen.getByTitle("Spotify");
		expect(link.getAttribute("target")).toBe("_blank");
		expect(link.getAttribute("rel")).toContain("noopener");

		const event = fireEvent.click(link);
		expect(event).toBe(true);
		expect(bridge.openExternalUrl).not.toHaveBeenCalled();
	});

	it("hands the link to the system browser in the Desktop Client", () => {
		bridge.isDesktopClient.mockReturnValue(true);
		render(
			<ExternalLinkButton
				href="https://open.spotify.com/search/x"
				name="Spotify"
				short="SP"
			/>,
		);

		const notPrevented = fireEvent.click(screen.getByTitle("Spotify"));
		expect(notPrevented).toBe(false);
		expect(bridge.openExternalUrl).toHaveBeenCalledWith(
			"https://open.spotify.com/search/x",
		);
	});
});
