import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";

const bridge = vi.hoisted(() => ({
	isDesktopClient: vi.fn(),
	getServerConnection: vi.fn(),
	initializeMediaProxy: vi.fn(),
	testServerConnection: vi.fn(),
	saveServerConnection: vi.fn(),
}));

vi.mock("./bridge", () => bridge);

import { DesktopConnectionGate } from "./DesktopConnectionGate";

describe("DesktopConnectionGate", () => {
	afterEach(cleanup);

	beforeEach(() => {
		bridge.isDesktopClient.mockReturnValue(true);
		bridge.getServerConnection.mockReset();
		bridge.testServerConnection.mockReset();
		bridge.saveServerConnection.mockReset();
		bridge.initializeMediaProxy.mockReset();
		bridge.initializeMediaProxy.mockResolvedValue(
			"http://127.0.0.1:41000/token",
		);
	});

	it("re-verifies a saved connection before rendering the application", async () => {
		bridge.getServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
		});
		bridge.testServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});

		render(<DesktopConnectionGate>application</DesktopConnectionGate>);

		expect(screen.getByText("Checking Music Server…")).toBeTruthy();
		expect(await screen.findByText("application")).toBeTruthy();
	});

	it("requires a successful test before saving and entering the application", async () => {
		bridge.getServerConnection.mockResolvedValue(null);
		bridge.testServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});
		bridge.saveServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});

		render(<DesktopConnectionGate>application</DesktopConnectionGate>);
		const input = await screen.findByLabelText("Music Server URL");
		fireEvent.change(input, { target: { value: "http://127.0.0.1:8090" } });

		const connect = screen.getByRole("button", { name: "Save and connect" });
		expect((connect as HTMLButtonElement).disabled).toBe(true);
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));

		await waitFor(() =>
			expect((connect as HTMLButtonElement).disabled).toBe(false),
		);
		fireEvent.click(connect);

		expect(await screen.findByText("application")).toBeTruthy();
		expect(bridge.saveServerConnection).toHaveBeenCalledWith(
			"http://127.0.0.1:8090",
		);
	});

	it("does not gate the Web Client", () => {
		bridge.isDesktopClient.mockReturnValue(false);

		render(<DesktopConnectionGate>application</DesktopConnectionGate>);

		expect(screen.getByText("application")).toBeTruthy();
		expect(bridge.getServerConnection).not.toHaveBeenCalled();
	});

	it("uses canonical origin returned by the Rust transport", async () => {
		bridge.getServerConnection.mockResolvedValue(null);
		bridge.testServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});

		render(<DesktopConnectionGate>application</DesktopConnectionGate>);
		const input = await screen.findByLabelText("Music Server URL");
		fireEvent.change(input, { target: { value: "http://localhost:8090" } });
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));

		await waitFor(() => {
			expect((input as HTMLInputElement).value).toBe("http://127.0.0.1:8090");
		});
		expect(
			(
				screen.getByRole("button", {
					name: "Save and connect",
				}) as HTMLButtonElement
			).disabled,
		).toBe(false);
	});
});
