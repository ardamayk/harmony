import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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
import { ImportSessionContext } from "#/components/import-session-context";
import { ImportSettingsSection } from "./-import-settings";

const mocks = vi.hoisted(() => ({ getHealth: vi.fn() }));

vi.mock("#/lib/api", () => ({ apiClient: { getHealth: mocks.getHealth } }));

function renderSection() {
	const open = vi.fn();
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	render(
		<QueryClientProvider client={queryClient}>
			<ImportSessionContext.Provider value={{ open }}>
				<ImportSettingsSection />
			</ImportSessionContext.Provider>
		</QueryClientProvider>,
	);
	return { open, button: screen.getByRole("button", { name: "Import Music" }) };
}

describe("ImportSettingsSection", () => {
	beforeEach(() => {
		mocks.getHealth.mockReset();
	});

	afterEach(cleanup);

	it("opens Managed Import from a labelled control", async () => {
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "test",
			capabilities: ["api.v1", "managed-import.v1"],
		});
		const { open, button } = renderSection();
		await waitFor(() => expect(mocks.getHealth).toHaveBeenCalled());

		expect(
			screen.getByRole("region", { name: "Library" }).contains(button),
		).toBe(true);
		expect(button.hasAttribute("disabled")).toBe(false);
		fireEvent.click(button);
		expect(open).toHaveBeenCalledOnce();
	});

	it("stays usable while the Music Server has not answered", () => {
		mocks.getHealth.mockReturnValue(new Promise(() => {}));
		const { open, button } = renderSection();

		expect(button.hasAttribute("disabled")).toBe(false);
		fireEvent.click(button);
		expect(open).toHaveBeenCalledOnce();
	});

	it("disables Import Music when the Music Server lacks the Managed Import capability", async () => {
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "legacy",
			capabilities: ["api.v1", "future.unknown-feature.v2"],
		});
		const { open, button } = renderSection();

		await waitFor(() => expect(button.hasAttribute("disabled")).toBe(true));
		expect(button.getAttribute("title")).toContain("Managed Import");
		fireEvent.click(button);
		expect(open).not.toHaveBeenCalled();
	});

	it("keeps Import Music available when the Music Server advertises extra unknown capabilities", async () => {
		mocks.getHealth.mockResolvedValue({
			status: "ok",
			version: "newer",
			capabilities: [
				"api.v1",
				"managed-import.v1",
				"future.unknown-feature.v2",
			],
		});
		const { open, button } = renderSection();
		await waitFor(() => expect(mocks.getHealth).toHaveBeenCalled());

		expect(button.hasAttribute("disabled")).toBe(false);
		fireEvent.click(button);
		expect(open).toHaveBeenCalledOnce();
	});
});
