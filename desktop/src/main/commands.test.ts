import { describe, expect, it, vi } from "vite-plus/test";
import {
	type CommandDeps,
	handleInvoke,
	runWindowAction,
	type WindowControls,
} from "./commands";
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

	it("refuses invalid arguments without reaching the core or the shell", async () => {
		const shell = deps();
		const cases: [string, unknown][] = [
			// Missing, mistyped and extra fields.
			["desktop_playback_seek", undefined],
			["desktop_playback_seek", {}],
			["desktop_playback_seek", { seconds: "42" }],
			["desktop_playback_seek", { seconds: 42, paths: ["/etc/passwd"] }],
			["desktop_playback_set_transition_fade", { milliseconds: -1 }],
			["desktop_playback_set_transition_fade", { milliseconds: 1.5 }],
			["desktop_playback_set_equalizer_gain", { index: 0 }],
			["desktop_playback_set_replay_gain", { mode: "loud" }],
			["desktop_playback_set_processing_profile", { profile: 1 }],
			["desktop_playback_sync_queue_context", { sources: {}, currentIndex: 0 }],
			[
				"desktop_playback_play",
				{ source: { type: "file", playbackUrl: "/etc/passwd" } },
			],
			["desktop_release_import_selections", { selectionIds: [1, 2] }],
			["save_server_connection", "http://example.com"],
			[
				"desktop_http_request",
				{ request: { method: "GET", url: "/", body: [256] } },
			],
			["desktop_http_request", { request: { method: "GET", url: "/" } }],
			// Commands without arguments take none.
			["desktop_playback_pause", { force: true }],
			["desktop_playback_pause", "now"],
			["desktop_select_import_files", { paths: ["/etc/passwd"] }],
			["desktop_open_external_url", { url: 42 }],
			["desktop_open_external_url", undefined],
			["desktop_show_main_window", []],
		];
		for (const [command, args] of cases) {
			const result = await handleInvoke(shell, command, args);
			expect(result.ok, `${command} ${JSON.stringify(args)}`).toBe(false);
		}
		expect(shell.requestCore).not.toHaveBeenCalled();
		expect(shell.pickImportFiles).not.toHaveBeenCalled();
		expect(shell.openExternal).not.toHaveBeenCalled();
		expect(shell.showMainWindow).not.toHaveBeenCalled();
	});

	it("names the command and the offending fields, not their values", async () => {
		const result = await handleInvoke(deps(), "desktop_playback_seek", {
			seconds: "secret-value",
			extra: "other-secret",
		});

		expect(result.ok).toBe(false);
		const error = String(!result.ok && result.error);
		expect(error).toContain("desktop_playback_seek");
		expect(error).toContain("seconds");
		expect(error).toContain("extra");
		expect(error).not.toContain("secret");
	});

	it("accepts the arguments the renderer sends", async () => {
		const requestCore = vi.fn<CommandDeps["requestCore"]>();
		const source = {
			type: "track",
			track: { id: "t1", title: "Song" },
			playbackUrl: "http://127.0.0.1:43129/token/tracks/t1",
			queueItemId: "q1",
		};
		const cases: [string, unknown][] = [
			["get_server_connection", undefined],
			["get_server_connection", {}],
			["desktop_playback_play", { source }],
			["desktop_playback_play", { source: undefined }],
			[
				"desktop_playback_sync_queue_context",
				{ sources: [source], currentIndex: null },
			],
			["desktop_playback_set_equalizer_gain", { index: 3, value: -2.5 }],
			["desktop_playback_set_equalizer_preset", { preset: "bass-boost" }],
			[
				"desktop_http_request",
				{
					request: {
						method: "PATCH",
						url: "/api/v1/preferences",
						headers: { "content-type": "application/json" },
						body: [123, 125],
					},
				},
			],
			[
				"desktop_upload_import_file",
				{ selectionId: "s1", uploadId: "u1", jobId: "j1" },
			],
		];
		for (const [command, args] of cases) {
			const result = await handleInvoke(deps({ requestCore }), command, args);
			expect(result.ok, `${command} ${JSON.stringify(args)}`).toBe(true);
		}
		expect(requestCore).toHaveBeenCalledWith("desktop_playback_play", {
			source,
		});
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
		const request = {
			method: "GET",
			url: "/api/v1/external/search?q=a",
			body: null,
		};
		// A UUID, as the renderer generates it.
		const requestId = "3f0c1e9a-5b7d-4c2e-9a1f-6d8b2e4c7a90";

		const pending = handleInvoke(shell, "desktop_http_request", {
			request,
			requestId,
		});
		const cancel = await handleInvoke(shell, "desktop_cancel_http_request", {
			requestId,
		});

		expect(cancel).toEqual({ ok: true, value: null });
		// Sent in this order, so the core has registered the request when the
		// cancel arrives.
		expect(requestCore.mock.calls).toEqual([
			["desktop_http_request", { request, requestId }],
			["desktop_cancel_http_request", { requestId }],
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

describe("runWindowAction", () => {
	function controls(maximized = false): WindowControls {
		return {
			minimize: vi.fn(),
			maximize: vi.fn(),
			unmaximize: vi.fn(),
			isMaximized: vi.fn(() => maximized),
			close: vi.fn(),
		};
	}

	it("runs the renderer's window actions", () => {
		const window = controls();
		runWindowAction(window, "minimize");
		runWindowAction(window, "toggle-maximize");
		runWindowAction(window, "close");

		expect(window.minimize).toHaveBeenCalledTimes(1);
		expect(window.maximize).toHaveBeenCalledTimes(1);
		expect(window.close).toHaveBeenCalledTimes(1);

		const maximized = controls(true);
		runWindowAction(maximized, "toggle-maximize");
		expect(maximized.unmaximize).toHaveBeenCalledTimes(1);
	});

	it("drops anything else", () => {
		const window = controls();
		for (const action of [
			"maximize",
			"Close",
			"",
			1,
			null,
			{ action: "close" },
		]) {
			runWindowAction(window, action);
		}
		for (const control of Object.values(window)) {
			expect(control).not.toHaveBeenCalled();
		}
	});
});
