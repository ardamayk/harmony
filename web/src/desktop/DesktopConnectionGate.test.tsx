import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { useEffect } from "react";
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

function renderGate(queryClient = new QueryClient()) {
	return render(
		<QueryClientProvider client={queryClient}>
			<DesktopConnectionGate>application</DesktopConnectionGate>
		</QueryClientProvider>,
	);
}

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

		renderGate();

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

		renderGate();
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
			undefined,
		);
	});

	it("opens on the saved lists when the saved server does not answer, and tries again", async () => {
		bridge.getServerConnection.mockResolvedValue({
			origin: "http://192.168.1.5:8090",
		});
		bridge.testServerConnection.mockRejectedValueOnce({
			code: "unreachable",
			message: "Music Server is unreachable.",
		});
		const queryClient = new QueryClient();
		const invalidate = vi.spyOn(queryClient, "invalidateQueries");

		renderGate(queryClient);

		expect(await screen.findByText("application")).toBeTruthy();
		expect(screen.getByRole("status").textContent).toContain(
			"Showing saved lists from http://192.168.1.5:8090",
		);
		expect(bridge.initializeMediaProxy).toHaveBeenCalled();

		bridge.testServerConnection.mockResolvedValueOnce({
			origin: "http://192.168.1.5:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});
		fireEvent.click(screen.getByRole("button", { name: "Try again" }));

		await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
		expect(invalidate).toHaveBeenCalled();
		expect(screen.getByText("application")).toBeTruthy();
	});

	it("mounts the application again when the saved server comes back", async () => {
		bridge.getServerConnection.mockResolvedValue({
			origin: "http://192.168.1.5:8090",
		});
		bridge.testServerConnection.mockRejectedValueOnce({
			code: "unreachable",
			message: "Music Server is unreachable.",
		});
		const onMount = vi.fn();
		function Application() {
			useEffect(onMount, []);
			return "application";
		}

		render(
			<QueryClientProvider client={new QueryClient()}>
				<DesktopConnectionGate>
					<Application />
				</DesktopConnectionGate>
			</QueryClientProvider>,
		);
		await screen.findByRole("status");
		expect(onMount).toHaveBeenCalledOnce();

		bridge.testServerConnection.mockResolvedValueOnce({
			origin: "http://192.168.1.5:8090",
			version: "0.1.0",
			capabilities: ["api.v1"],
		});
		fireEvent.click(screen.getByRole("button", { name: "Try again" }));

		await waitFor(() => expect(onMount).toHaveBeenCalledTimes(2));
	});

	it("keeps the connection form for a server that answers but does not fit", async () => {
		bridge.getServerConnection.mockResolvedValue({
			origin: "http://127.0.0.1:8090",
		});
		bridge.testServerConnection.mockRejectedValueOnce({
			code: "capability_mismatch",
			message: "Music Server lacks required capabilities.",
		});

		renderGate();

		expect(
			await screen.findByText("Music Server lacks required capabilities."),
		).toBeTruthy();
		expect(screen.queryByText("application")).toBeNull();
	});

	it("does not gate the Web Client", () => {
		bridge.isDesktopClient.mockReturnValue(false);

		renderGate();

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

		renderGate();
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

	it("sends custom headers for an https server, and none for loopback", async () => {
		bridge.getServerConnection.mockResolvedValue(null);
		const origin = "https://music.example.com";
		bridge.testServerConnection.mockResolvedValue({
			origin,
			version: "1",
			capabilities: [],
		});
		bridge.saveServerConnection.mockResolvedValue({ origin });
		renderGate();

		const url = await screen.findByLabelText("Music Server URL");
		expect(screen.queryByLabelText("Header name")).toBeNull();
		fireEvent.change(url, { target: { value: origin } });
		fireEvent.change(screen.getByLabelText("Header name"), {
			target: { value: "X-Proxy-Key" },
		});
		fireEvent.change(screen.getByLabelText("Header value"), {
			target: { value: "secret" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
		await screen.findByText(/Connected to Music Server/);
		fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));

		await screen.findByText("application");
		const headers = { "X-Proxy-Key": "secret" };
		expect(bridge.testServerConnection).toHaveBeenCalledWith(origin, headers);
		expect(bridge.saveServerConnection).toHaveBeenCalledWith(origin, headers);
	});

	it("leaves saved headers alone when no row is filled in", async () => {
		bridge.getServerConnection.mockResolvedValue(null);
		const origin = "https://music.example.com";
		bridge.testServerConnection.mockRejectedValue({
			code: "access_denied",
			message: "The server refused this client.",
		});
		renderGate();

		fireEvent.change(await screen.findByLabelText("Music Server URL"), {
			target: { value: origin },
		});
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));

		await screen.findByText("The server refused this client.");
		expect(bridge.testServerConnection).toHaveBeenCalledWith(origin, undefined);
	});
});
