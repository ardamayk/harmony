import { describe, expect, it, vi } from "vite-plus/test";
import { type CommandDeps, handleInvoke } from "./commands";
import { CoreCommandError } from "./core-process";

function deps(overrides: Partial<CommandDeps> = {}): CommandDeps {
	return {
		requestCore: vi.fn<CommandDeps["requestCore"]>().mockResolvedValue(null),
		pickImportFiles: vi
			.fn<CommandDeps["pickImportFiles"]>()
			.mockResolvedValue([]),
		pickImportFolder: vi
			.fn<CommandDeps["pickImportFolder"]>()
			.mockResolvedValue([]),
		openExternal: vi.fn<CommandDeps["openExternal"]>().mockResolvedValue(),
		openMiniWindow: vi.fn(),
		closeMiniWindow: vi.fn(),
		toggleMiniWindow: vi.fn(),
		showMainWindow: vi.fn(),
		...overrides,
	};
}

describe("handleInvoke", () => {
	it("forwards allowlisted core commands with their arguments", async () => {
		const requestCore = vi
			.fn<CommandDeps["requestCore"]>()
			.mockResolvedValue({ status: "playing" });
		const result = await handleInvoke(
			deps({ requestCore }),
			"desktop_playback_seek",
			{ seconds: 42 },
		);

		expect(requestCore).toHaveBeenCalledWith("desktop_playback_seek", {
			seconds: 42,
		});
		expect(result).toEqual({ ok: true, value: { status: "playing" } });
	});

	it("refuses commands outside the allowlist without reaching the core", async () => {
		const requestCore = vi.fn<CommandDeps["requestCore"]>();
		for (const command of [
			"desktop_register_import_paths",
			"shell_action",
			"anything",
			42,
		]) {
			const result = await handleInvoke(deps({ requestCore }), command, {
				paths: ["/etc/passwd"],
			});
			expect(result.ok).toBe(false);
		}
		expect(requestCore).not.toHaveBeenCalled();
	});

	it("passes the core's own error value through", async () => {
		const requestCore = vi
			.fn<CommandDeps["requestCore"]>()
			.mockRejectedValue(
				new CoreCommandError({ code: "invalid_origin", message: "Nope" }),
			);
		const result = await handleInvoke(
			deps({ requestCore }),
			"save_server_connection",
			{ origin: "http://example.com" },
		);

		expect(result).toEqual({
			ok: false,
			error: { code: "invalid_origin", message: "Nope" },
		});
	});

	it("forwards an HTTP cancel to the core while the request is in flight", async () => {
		let finishRequest: ((value: unknown) => void) | undefined;
		const requestCore = vi
			.fn<CommandDeps["requestCore"]>()
			.mockImplementation((method) =>
				method === "desktop_http_request"
					? new Promise((resolve) => {
							finishRequest = resolve;
						})
					: Promise.resolve(null),
			);
		const shell = deps({ requestCore });
		const request = { method: "GET", url: "/api/v1/external/search?q=a" };

		const pending = handleInvoke(shell, "desktop_http_request", {
			request,
			requestId: "r1",
		});
		const cancel = await handleInvoke(shell, "desktop_cancel_http_request", {
			requestId: "r1",
		});

		expect(cancel).toEqual({ ok: true, value: null });
		// Sent in this order, so the core has registered the request when the
		// cancel arrives.
		expect(requestCore.mock.calls).toEqual([
			["desktop_http_request", { request, requestId: "r1" }],
			["desktop_cancel_http_request", { requestId: "r1" }],
		]);
		finishRequest?.(null);
		await pending;
	});

	it("registers picked import paths with the core and returns its selections", async () => {
		const requestCore = vi
			.fn<CommandDeps["requestCore"]>()
			.mockResolvedValue([{ id: "s1", name: "a.flac", size: 3 }]);
		const pickImportFiles = vi
			.fn<CommandDeps["pickImportFiles"]>()
			.mockResolvedValue(["/music/a.flac"]);
		const result = await handleInvoke(
			deps({ requestCore, pickImportFiles }),
			"desktop_select_import_files",
			undefined,
		);

		expect(requestCore).toHaveBeenCalledWith("desktop_register_import_paths", {
			paths: ["/music/a.flac"],
		});
		expect(result).toEqual({
			ok: true,
			value: [{ id: "s1", name: "a.flac", size: 3 }],
		});
	});

	it("returns no selections when the picker is canceled", async () => {
		const requestCore = vi.fn<CommandDeps["requestCore"]>();
		const result = await handleInvoke(
			deps({ requestCore }),
			"desktop_select_import_folder",
			undefined,
		);

		expect(result).toEqual({ ok: true, value: [] });
		expect(requestCore).not.toHaveBeenCalled();
	});

	it("opens only web URLs externally", async () => {
		const openExternal = vi
			.fn<CommandDeps["openExternal"]>()
			.mockResolvedValue();
		const allowed = await handleInvoke(
			deps({ openExternal }),
			"desktop_open_external_url",
			{ url: "https://www.last.fm/music/x" },
		);
		const refused = await handleInvoke(
			deps({ openExternal }),
			"desktop_open_external_url",
			{ url: "file:///etc/passwd" },
		);

		expect(allowed).toEqual({ ok: true, value: null });
		expect(refused.ok).toBe(false);
		expect(openExternal).toHaveBeenCalledTimes(1);
		expect(openExternal).toHaveBeenCalledWith("https://www.last.fm/music/x");
	});

	it("runs window commands in the shell", async () => {
		const shell = deps();
		await handleInvoke(shell, "desktop_toggle_mini_player", undefined);
		await handleInvoke(shell, "desktop_show_main_window", undefined);

		expect(shell.toggleMiniWindow).toHaveBeenCalledTimes(1);
		expect(shell.showMainWindow).toHaveBeenCalledTimes(1);
		expect(shell.requestCore).not.toHaveBeenCalled();
	});
});
