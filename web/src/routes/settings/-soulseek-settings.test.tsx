import { ApiError, type SoulseekStatus } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { SoulseekSettingsSection } from "./-soulseek-settings";

const mocks = vi.hoisted(() => ({
	hasSoulseek: true,
	getSoulseekStatus: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: { getSoulseekStatus: mocks.getSoulseekStatus },
}));
vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: (capability: string) =>
		capability === "soulseek-acquisition.v1" && mocks.hasSoulseek,
}));

const online: SoulseekStatus = {
	slskd: { reachable: true, version: "0.22.5" },
	soulseek: { connected: true, loggedIn: true, username: "earthly-op" },
	sharing: { state: "shared", sharedFileCount: 12_345 },
};

function renderSection() {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	render(
		<QueryClientProvider client={client}>
			<SoulseekSettingsSection />
		</QueryClientProvider>,
	);
}

describe("SoulseekSettingsSection", () => {
	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
		mocks.hasSoulseek = true;
	});

	it("is absent without the capability", () => {
		mocks.hasSoulseek = false;
		renderSection();

		expect(screen.queryByRole("region", { name: "Soulseek" })).toBeNull();
		expect(mocks.getSoulseekStatus).not.toHaveBeenCalled();
	});

	it("shows slskd, the Soulseek login, sharing and the operator's responsibility", async () => {
		mocks.getSoulseekStatus.mockResolvedValue(online);
		renderSection();

		const section = screen.getByRole("region", { name: "Soulseek" });
		expect(await screen.findByText("Reachable · version 0.22.5")).toBeTruthy();
		expect(screen.getByText("Logged in as earthly-op")).toBeTruthy();
		expect(
			screen.getByText(
				`Your library is shared · ${(12_345).toLocaleString()} files`,
			),
		).toBeTruthy();
		expect(section.textContent).toMatch(
			/Downloading or sharing copyrighted music without permission is unlawful in many places/,
		);
		expect(section.textContent).toMatch(
			/responsible for what it downloads and shares/,
		);
	});

	it("says how the operator shares Managed Storage when it is not shared", async () => {
		mocks.getSoulseekStatus.mockResolvedValue({
			...online,
			sharing: { state: "not_shared" },
		});
		renderSection();

		expect(await screen.findByText("Your library is not shared.")).toBeTruthy();
		expect(
			screen.getByText(
				"The operator shares Managed Storage in slskd’s configuration.",
			),
		).toBeTruthy();
		expect(screen.queryByRole("switch")).toBeNull();
	});

	it("shows an unreachable slskd with unknown login and sharing", async () => {
		mocks.getSoulseekStatus.mockResolvedValue({
			slskd: { reachable: false },
			soulseek: { connected: false, loggedIn: false },
			sharing: { state: "unknown" },
		});
		renderSection();

		expect(await screen.findByText(/^Unreachable\./)).toBeTruthy();
		expect(screen.getAllByText("Unknown")).toHaveLength(2);
	});

	it("shows a logged-out Soulseek connection", async () => {
		mocks.getSoulseekStatus.mockResolvedValue({
			...online,
			soulseek: { connected: true, loggedIn: false },
		});
		renderSection();

		expect(await screen.findByText("Connected, not logged in")).toBeTruthy();
	});

	it("explains a failed status request", async () => {
		mocks.getSoulseekStatus.mockRejectedValue(
			new ApiError(503, {
				error: "unreachable",
				code: "slskd_unreachable",
				message: "unreachable",
			}),
		);
		renderSection();

		expect((await screen.findByRole("alert")).textContent).toContain(
			"Couldn't reach slskd. Check that it is running, then retry.",
		);
	});
});
