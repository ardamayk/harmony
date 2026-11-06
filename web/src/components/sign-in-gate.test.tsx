import { ApiError } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	act,
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
import { markSignedIn, markSignedOut } from "#/lib/sign-in";

const mocks = vi.hoisted(() => ({
	login: vi.fn(),
	dispose: vi.fn(),
	isDesktopClient: vi.fn(() => false),
	getServerConnection: vi.fn(),
	testServerConnection: vi.fn(),
	saveServerConnection: vi.fn(),
}));

vi.mock("#/lib/api", () => ({ apiClient: { login: mocks.login } }));
vi.mock("#/playback/shared-playback-engine", () => ({
	stopSharedPlaybackEngine: mocks.dispose,
}));
vi.mock("#/desktop/bridge", () => ({
	isDesktopClient: mocks.isDesktopClient,
	getServerConnection: mocks.getServerConnection,
	testServerConnection: mocks.testServerConnection,
	saveServerConnection: mocks.saveServerConnection,
}));

import { SignInGate } from "./sign-in-gate";

function renderGate() {
	return render(
		<QueryClientProvider client={new QueryClient()}>
			<SignInGate>application</SignInGate>
		</QueryClientProvider>,
	);
}

async function signIn(username: string, password: string) {
	fireEvent.change(screen.getByLabelText("Username"), {
		target: { value: username },
	});
	fireEvent.change(screen.getByLabelText("Password"), {
		target: { value: password },
	});
	fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
}

const apiError = (status: number, code: string, retryAfterSeconds?: number) =>
	new ApiError(status, { error: code, code, message: code }, retryAfterSeconds);

describe("SignInGate", () => {
	beforeEach(() => {
		for (const mock of Object.values(mocks)) mock.mockReset();
		mocks.isDesktopClient.mockReturnValue(false);
	});

	afterEach(() => {
		cleanup();
		markSignedIn();
	});

	it("shows the app until the Music Server asks for sign-in", () => {
		renderGate();
		expect(screen.getByText("application")).toBeTruthy();
		expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();

		act(markSignedOut);

		expect(screen.queryByText("application")).toBeNull();
		expect(screen.getByLabelText("Username").getAttribute("autocomplete")).toBe(
			"username",
		);
		expect(screen.getByLabelText("Password").getAttribute("autocomplete")).toBe(
			"current-password",
		);
		expect(mocks.dispose).toHaveBeenCalledOnce();
	});

	it("continues into the app after signing in", async () => {
		mocks.login.mockResolvedValue(undefined);
		markSignedOut();
		renderGate();

		await signIn("owner", "secret");

		await screen.findByText("application");
		expect(mocks.login).toHaveBeenCalledWith({
			username: "owner",
			password: "secret",
		});
	});

	it.each([
		[apiError(401, "invalid_credentials"), "Wrong username or password."],
		[
			apiError(429, "too_many_attempts", 120),
			"Too many failed attempts. Try again in 2 minutes.",
		],
		[
			apiError(429, "too_many_attempts", 1),
			"Too many failed attempts. Try again in 1 second.",
		],
		[new TypeError("Failed to fetch"), "Couldn't reach the Music Server."],
	])("explains a failed sign-in: %s", async (error, message) => {
		mocks.login.mockRejectedValue(error);
		markSignedOut();
		renderGate();

		await signIn("owner", "wrong");

		const alert = await screen.findByRole("alert");
		expect(alert.textContent).toContain(message);
		expect(screen.queryByText("application")).toBeNull();
		expect(
			screen.getByRole("button", { name: "Sign in" }).hasAttribute("disabled"),
		).toBe(false);
	});

	it("saves Basic credentials as a header on the Desktop Client", async () => {
		mocks.isDesktopClient.mockReturnValue(true);
		mocks.getServerConnection.mockResolvedValue({
			origin: "https://music.example.com",
			headerNames: [],
		});
		const check = {
			origin: "https://music.example.com",
			version: "0.1.0",
			capabilities: ["api.v1", "auth.session.v1"],
		};
		mocks.testServerConnection.mockResolvedValue(check);
		mocks.saveServerConnection.mockReturnValue(new Promise(() => {}));
		markSignedOut();
		renderGate();

		fireEvent.change(await screen.findByLabelText("Username"), {
			target: { value: "owner" },
		});
		fireEvent.change(screen.getByLabelText("Password"), {
			target: { value: "sécret" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
		const save = await screen.findByRole("button", {
			name: "Save and connect",
		});
		await waitFor(() => expect(save.hasAttribute("disabled")).toBe(false));
		fireEvent.click(save);

		const basic = `Basic ${btoa(String.fromCharCode(...new TextEncoder().encode("owner:sécret")))}`;
		await waitFor(() =>
			expect(mocks.saveServerConnection).toHaveBeenCalledWith(
				"https://music.example.com",
				{ Authorization: basic },
				// Kept beside a proxy's saved headers, not in their place.
				true,
			),
		);
	});
});
