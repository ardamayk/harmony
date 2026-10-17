import {
	type InvokeResult,
	isCoreCommand,
	isDesktopCommand,
	type ShellCommand,
} from "@repo/desktop-bridge";
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
 * and lifetime actions.
 */
export async function handleInvoke(
	deps: CommandDeps,
	command: unknown,
	args: unknown,
): Promise<InvokeResult> {
	try {
		if (!isDesktopCommand(command)) {
			throw new CoreCommandError(
				`Desktop command '${String(command)}' is not allowed.`,
			);
		}
		const params = isPlainObject(args) ? args : undefined;
		const value = isCoreCommand(command)
			? await deps.requestCore(command, params)
			: await runShellCommand(deps, command, params);
		return { ok: true, value: value ?? null };
	} catch (error) {
		return { ok: false, error: errorValue(error) };
	}
}

async function runShellCommand(
	deps: CommandDeps,
	command: ShellCommand,
	params: Record<string, unknown> | undefined,
): Promise<unknown> {
	switch (command) {
		case "desktop_select_import_files":
			return registerImportPaths(deps, await deps.pickImportFiles());
		case "desktop_select_import_folder":
			return registerImportPaths(deps, await deps.pickImportFolder());
		case "desktop_open_external_url":
			return deps.openExternal(validateExternalUrl(params?.url));
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

function isPlainObject(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}
