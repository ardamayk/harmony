import { DESKTOP_EVENTS, type DesktopUpdateStatus } from "@repo/desktop-bridge";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	type FakeDesktopBridge,
	installDesktopBridge,
} from "#/desktop/testing";
import { DesktopUpdateSection } from "./-desktop-update-settings";

const CURRENT = { currentVersion: "0.1.17" };
const VERSION = "0.1.18";

let fake: FakeDesktopBridge | undefined;

async function renderSection(initial: DesktopUpdateStatus) {
	const handle = vi.fn((command: string) =>
		command === "desktop_get_update_status" ? initial : null,
	);
	fake = installDesktopBridge(handle);
	render(
		<QueryClientProvider client={new QueryClient()}>
			<DesktopUpdateSection />
		</QueryClientProvider>,
	);
	await screen.findByText("0.1.17");
	return handle;
}

function emit(status: DesktopUpdateStatus) {
	act(() => fake?.emit(DESKTOP_EVENTS.updateStatus, status));
}

function press(name: string) {
	fireEvent.click(screen.getByRole("button", { name }));
}

describe("DesktopUpdateSection", () => {
	afterEach(() => {
		cleanup();
		fake?.uninstall();
		fake = undefined;
	});

	it("shows the version and checks when asked", async () => {
		const handle = await renderSection({ ...CURRENT, status: "idle" });

		press("Check for updates");

		expect(handle).toHaveBeenCalledWith("desktop_check_for_updates", undefined);
	});

	it("shows a running check as busy", async () => {
		await renderSection({ ...CURRENT, status: "idle" });

		emit({ ...CURRENT, status: "checking" });

		const button = await screen.findByRole<HTMLButtonElement>("button", {
			name: "Checking for updates",
		});
		expect(button.disabled).toBe(true);
	});

	it("walks from the offer through the download to the restart", async () => {
		const handle = await renderSection({
			...CURRENT,
			status: "available",
			version: VERSION,
		});
		expect(screen.getByText(`Version ${VERSION} is available.`)).toBeTruthy();

		press("Download update");
		expect(handle).toHaveBeenCalledWith("desktop_download_update", undefined);

		emit({ ...CURRENT, status: "downloading", version: VERSION, percent: 42 });
		expect(await screen.findByText("42%")).toBeTruthy();
		expect(screen.getByRole("progressbar")).toBeTruthy();
		expect(screen.queryByRole("button")).toBeNull();
		expect(handle).not.toHaveBeenCalledWith(
			"desktop_install_update",
			undefined,
		);

		emit({ ...CURRENT, status: "downloaded", version: VERSION });
		fireEvent.click(
			await screen.findByRole("button", { name: "Restart to update" }),
		);
		expect(handle).toHaveBeenCalledWith("desktop_install_update", undefined);

		emit({ ...CURRENT, status: "installing", version: VERSION });
		const button = await screen.findByRole<HTMLButtonElement>("button", {
			name: "Restarting to update",
		});
		expect(button.disabled).toBe(true);
	});

	it("says a failed check can be retried", async () => {
		const handle = await renderSection({
			...CURRENT,
			status: "failed",
			action: "check",
		});

		expect(screen.getByRole("alert").textContent).toContain(
			"Couldn't check for updates",
		);
		press("Retry check");
		expect(handle).toHaveBeenCalledWith("desktop_check_for_updates", undefined);
	});

	it("says a failed download can be retried", async () => {
		const handle = await renderSection({
			...CURRENT,
			status: "failed",
			action: "download",
			version: VERSION,
		});

		expect(screen.getByRole("alert").textContent).toContain(
			`Couldn't download version ${VERSION}`,
		);
		press("Retry download");
		expect(handle).toHaveBeenCalledWith("desktop_download_update", undefined);
	});

	it("offers no action where the client cannot update", async () => {
		await renderSection({ ...CURRENT, status: "unavailable" });

		expect(screen.getByText(/Only installed releases/)).toBeTruthy();
		expect(screen.queryByRole("button")).toBeNull();
	});
});
