import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

const mocks = vi.hoisted(() => ({
	logout: vi.fn(),
	markSignedOut: vi.fn(),
}));

vi.mock("#/lib/api", () => ({ apiClient: { logout: mocks.logout } }));
vi.mock("#/lib/sign-in", () => ({ markSignedOut: mocks.markSignedOut }));

import { AccountSection } from "./-account-settings";

const health = (capabilities: string[]) => ({
	status: "ok" as const,
	version: "0.1.0",
	capabilities,
	dependencies: [],
});

function renderSection(capabilities: string[]) {
	return render(
		<QueryClientProvider client={new QueryClient()}>
			<AccountSection health={health(capabilities)} />
		</QueryClientProvider>,
	);
}

describe("AccountSection", () => {
	afterEach(cleanup);

	it("offers Sign out only when the Music Server has sign-in", () => {
		renderSection(["api.v1"]);
		expect(screen.queryByRole("button", { name: "Sign out" })).toBeNull();
		cleanup();

		renderSection(["api.v1", "auth.session.v1"]);
		expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
	});

	it("signs out and returns to the sign-in screen", async () => {
		mocks.logout.mockResolvedValue(undefined);
		renderSection(["api.v1", "auth.session.v1"]);

		fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

		await waitFor(() => expect(mocks.markSignedOut).toHaveBeenCalledOnce());
		expect(mocks.logout).toHaveBeenCalledOnce();
	});
});
