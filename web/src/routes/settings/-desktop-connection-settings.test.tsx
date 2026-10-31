import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { DesktopConnectionSection } from "./-desktop-connection-settings";

const bridge = vi.hoisted(() => ({
	getServerConnection: vi.fn(),
	saveServerConnection: vi.fn(),
	testServerConnection: vi.fn(),
}));

vi.mock("#/desktop/bridge", () => bridge);

const ORIGIN = "https://music.example.com";

function renderSection() {
	bridge.getServerConnection.mockResolvedValue({
		origin: ORIGIN,
		headerNames: ["x-proxy-key"],
	});
	return render(
		<QueryClientProvider client={new QueryClient()}>
			<DesktopConnectionSection />
		</QueryClientProvider>,
	);
}

describe("DesktopConnectionSection", () => {
	afterEach(cleanup);

	it("shows the URL and only the names of the custom headers", async () => {
		renderSection();

		expect(await screen.findByText(ORIGIN)).toBeTruthy();
		expect(screen.getByText("x-proxy-key")).toBeTruthy();
		expect(screen.queryByLabelText("Header value")).toBeNull();
	});

	it("keeps saved headers unless they are replaced or removed", async () => {
		bridge.testServerConnection.mockResolvedValue({
			origin: ORIGIN,
			version: "1",
			capabilities: [],
		});
		renderSection();
		await screen.findByText(ORIGIN);
		fireEvent.click(
			screen.getByRole("button", { name: "Change Music Server" }),
		);

		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
		await screen.findByText(/Connected to Music Server/);
		expect(bridge.testServerConnection).toHaveBeenLastCalledWith(
			ORIGIN,
			undefined,
		);

		fireEvent.click(
			screen.getByRole("button", { name: "Remove saved headers" }),
		);
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
		await screen.findByText(/Connected to Music Server/);
		expect(bridge.testServerConnection).toHaveBeenLastCalledWith(ORIGIN, {});
	});
});
