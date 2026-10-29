import {
	type InvokeResult,
	isCoreCommand,
	type ShellCommand,
} from "@repo/desktop-bridge";
import {
	type ParsedInvoke,
	parseInvoke,
	WINDOW_ACTION,
} from "@repo/desktop-bridge/schemas";
import { CoreCommandError } from "./core-process";
import { validateExternalUrl } from "./security";

/** Audio files the native picker offers; the core re-checks each one. */
export const IMPORT_EXTENSIONS = ["flac", "mp3", "m4a", "ogg", "opus", "wav"];

export type CommandDeps = {
	requestCore(
		method: string,
		params?: Record<string, unknown>,
	): Promise<unknown>;
	/** Native pickers; an empty list when the user cancels. */
	pickImportFiles(): Promise<string[]>;
	pickImportFolder(): Promise<string[]>;
	openExternal(url: string): Promise<void>;
	openMiniWindow(): void;
	closeMiniWindow(): void;
	toggleMiniWindow(): void;
	showMainWindow(): void;
};

/**
 * Answers one renderer invoke. Allowlisted core commands are forwarded with
 * their arguments; shell commands run here. Everything else is refused, so
 * renderer content cannot register file paths with the core or drive tray
 * and lifetime actions. Arguments must match the command's schema (see
 * DESKTOP_COMMAND_ARGS): missing, mistyped or extra fields are refused
 * before anything runs.
 */
export async function handleInvoke(
	deps: CommandDeps,
	command: unknown,
	args: unknown,
): Promise<InvokeResult> {
	try {
		const parsed = parseInvoke(command, args);
		if (!parsed.ok) throw new CoreCommandError(parsed.error);
		const invoke = parsed.value;
		const value = isCoreCommand(invoke.command)
			? await deps.requestCore(
					invoke.command,
					invoke.args as Record<string, unknown> | undefined,
				)
			: await runShellCommand(deps, invoke as ShellInvoke);
		return { ok: true, value: value ?? null };
	} catch (error) {
		return { ok: false, error: errorValue(error) };
	}
}

type ShellInvoke = Extract<ParsedInvoke, { command: ShellCommand }>;

async function runShellCommand(
	deps: CommandDeps,
	invoke: ShellInvoke,
): Promise<unknown> {
	switch (invoke.command) {
		case "desktop_select_import_files":
			return registerImportPaths(deps, await deps.pickImportFiles());
		case "desktop_select_import_folder":
			return registerImportPaths(deps, await deps.pickImportFolder());
		case "desktop_open_external_url":
			return deps.openExternal(validateExternalUrl(invoke.args.url));
		case "desktop_open_mini_player":
			return deps.openMiniWindow();
		case "desktop_close_mini_player":
			return deps.closeMiniWindow();
		case "desktop_toggle_mini_player":
			return deps.toggleMiniWindow();
		case "desktop_show_main_window":
			return deps.showMainWindow();
	}
}

/** The window controls the renderer's own frame drives. */
export type WindowControls = {
	minimize(): void;
	maximize(): void;
	unmaximize(): void;
	isMaximized(): boolean;
	close(): void;
};

/** Runs one window message from the renderer; anything else is dropped. */
export function runWindowAction(window: WindowControls, action: unknown): void {
	const parsed = WINDOW_ACTION.safeParse(action);
	if (!parsed.success) return;
	switch (parsed.data) {
		case "minimize":
			return window.minimize();
		case "toggle-maximize":
			return window.isMaximized() ? window.unmaximize() : window.maximize();
		case "close":
			return window.close();
	}
}

/** Hands picked paths to the core, which answers with opaque selection IDs. */
async function registerImportPaths(deps: CommandDeps, paths: string[]) {
	if (paths.length === 0) return [];
	return deps.requestCore("desktop_register_import_paths", { paths });
}

function errorValue(error: unknown): unknown {
	if (error instanceof CoreCommandError) return error.value;
	if (error instanceof Error) return error.message;
	return String(error);
}
