// Runtime schemas for every renderer → main message. The main process parses
// each message with these before it acts on it; the renderer shares the
// inferred types through `DesktopCommandArgs`. A separate entry so the
// sandboxed preload, which only needs the constants, does not bundle zod.

import * as z from "zod";
import type { DesktopCommand, WindowAction } from "./index";

/** No arguments: nothing, or an empty object. */
const noArgs = z.strictObject({}).optional();

const id = z.string();
/** A `crypto.randomUUID()` the renderer generates per cancellable request. */
const requestId = z.uuid();
const finite = z.number();
const nonNegativeInteger = z.int().nonnegative();

/**
 * A Playback Source, opaque to the core apart from the URL it plays (which
 * the core restricts to its media proxy). Its Track or station payload is
 * kept as the renderer sent it.
 */
const playbackSource = z.looseObject({
	type: z.enum(["track", "radio-station", "catalog-preview", "external-track"]),
	playbackUrl: z.string(),
});

/**
 * A Shared Playback State command, per the `/api/v1/playback/state/commands`
 * contract. The core passes it on unchecked, so this is where it is refused.
 */
const playbackStateCommand = z.discriminatedUnion("type", [
	z.strictObject({
		type: z.literal("claim"),
		deviceId: z.string().min(1).max(64),
		deviceName: z.string().min(1).max(64),
		queueItemId: id.nullable(),
		trackId: id.nullable(),
		positionMs: nonNegativeInteger,
		playing: z.boolean(),
	}),
	z.strictObject({
		type: z.literal("report"),
		deviceId: z.string().min(1).max(64),
		basedOnRevision: nonNegativeInteger,
		queueItemId: id.nullable(),
		trackId: id.nullable(),
		positionMs: nonNegativeInteger,
		playing: z.boolean(),
	}),
	z.strictObject({ type: z.literal("pause") }),
	z.strictObject({ type: z.literal("seek"), positionMs: nonNegativeInteger }),
	z.strictObject({
		type: z.literal("skip"),
		direction: z.enum(["next", "previous"]),
	}),
]);

/**
 * A request body as bytes. One check over the array instead of a schema per
 * element: an artwork upload is millions of them.
 */
const bytes = z.custom<number[]>(
	(value) =>
		Array.isArray(value) &&
		value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255),
	{ message: "Expected an array of bytes" },
);

/** Headers a proxy in front of the Music Server needs on every request. */
const serverHeaders = z.record(z.string(), z.string());

/**
 * The arguments of each command the renderer may send. Its keys are exactly
 * the allowlist (CORE_COMMANDS and SHELL_COMMANDS); extra fields are refused.
 */
export const DESKTOP_COMMAND_ARGS = {
	get_server_connection: noArgs,
	test_server_connection: z.strictObject({
		origin: z.string(),
		headers: serverHeaders.optional(),
	}),
	save_server_connection: z.strictObject({
		origin: z.string(),
		headers: serverHeaders.optional(),
	}),
	desktop_http_request: z.strictObject({
		request: z.strictObject({
			method: z.string(),
			url: z.string(),
			headers: z.record(z.string(), z.string()).optional(),
			body: bytes.nullable(),
		}),
		// Present when the renderer may cancel the request (it has a signal).
		requestId: requestId.optional(),
	}),
	desktop_cancel_http_request: z.strictObject({ requestId }),
	desktop_upload_import_file: z.strictObject({
		selectionId: id,
		uploadId: id,
		jobId: id,
	}),
	desktop_cancel_import_upload: z.strictObject({ uploadId: id }),
	desktop_release_import_selections: z.strictObject({
		selectionIds: z.array(id),
	}),
	get_media_proxy_url: noArgs,
	desktop_reconnect_queue_events: noArgs,
	get_desktop_playback_state: noArgs,
	get_desktop_mpv_status: noArgs,
	desktop_playback_renderer_ready: noArgs,
	desktop_playback_quit: noArgs,
	desktop_playback_play: z
		.strictObject({ source: playbackSource.optional() })
		.optional(),
	desktop_playback_pause: noArgs,
	desktop_playback_stop: noArgs,
	desktop_playback_toggle_play: noArgs,
	desktop_playback_sync_queue_context: z.strictObject({
		sources: z.array(playbackSource),
		currentIndex: nonNegativeInteger.nullable(),
	}),
	desktop_playback_previous: noArgs,
	desktop_playback_next: noArgs,
	desktop_playback_seek: z.strictObject({ seconds: finite }),
	desktop_playback_set_volume: z.strictObject({ value: finite }),
	desktop_playback_set_playback_rate: z.strictObject({ rate: finite }),
	desktop_playback_set_stop_after_current: z.strictObject({
		enabled: z.boolean(),
	}),
	desktop_playback_set_transition_fade: z.strictObject({
		milliseconds: nonNegativeInteger,
	}),
	desktop_playback_set_gapless: z.strictObject({ enabled: z.boolean() }),
	desktop_playback_set_processing_profile: z.strictObject({
		profile: z.enum(["direct", "processed"]),
	}),
	desktop_playback_set_replay_gain: z.strictObject({
		mode: z.enum(["off", "track", "album"]),
	}),
	desktop_playback_set_equalizer_preset: z.strictObject({
		preset: z.enum(["flat", "bass-boost", "vocal", "treble-boost", "custom"]),
	}),
	desktop_playback_set_equalizer_gain: z.strictObject({
		index: nonNegativeInteger,
		value: finite,
	}),
	desktop_playback_refresh_output_devices: noArgs,
	desktop_playback_select_direct_alsa_output: z.strictObject({
		deviceId: id,
	}),
	desktop_playback_select_exclusive_output: noArgs,
	desktop_playback_fallback_to_system_output: noArgs,
	desktop_playback_enable_adaptive_system_rate: noArgs,
	desktop_playback_toggle_shuffle: noArgs,
	desktop_playback_cycle_repeat_mode: noArgs,
	get_shared_playback_device: noArgs,
	get_shared_playback_state: noArgs,
	send_shared_playback_command: z.strictObject({
		command: playbackStateCommand,
	}),
	set_shared_playback_suspended: z.strictObject({ suspended: z.boolean() }),
	desktop_select_import_files: noArgs,
	desktop_select_import_folder: noArgs,
	// The URL itself is checked by the shell (web schemes only).
	desktop_open_external_url: z.strictObject({ url: z.string() }),
} as const satisfies Record<DesktopCommand, z.ZodType>;

export type DesktopCommandArgsMap = {
	[C in DesktopCommand]: z.input<(typeof DESKTOP_COMMAND_ARGS)[C]>;
};

export const WINDOW_ACTION = z.enum([
	"minimize",
	"toggle-maximize",
	"close",
]) satisfies z.ZodType<WindowAction>;

export type ParsedInvoke = {
	[C in DesktopCommand]: {
		command: C;
		args: z.output<(typeof DESKTOP_COMMAND_ARGS)[C]>;
	};
}[DesktopCommand];

export type InvokeParseResult =
	| { ok: true; value: ParsedInvoke }
	| { ok: false; error: string };

/**
 * Parses one renderer invoke: the command must be on the allowlist and its
 * arguments must match that command's schema. The error names the command
 * and the offending fields, never their values.
 */
export function parseInvoke(
	command: unknown,
	args: unknown,
): InvokeParseResult {
	if (
		typeof command !== "string" ||
		!Object.hasOwn(DESKTOP_COMMAND_ARGS, command)
	) {
		return {
			ok: false,
			error: `Desktop command '${String(command)}' is not allowed.`,
		};
	}
	const name = command as DesktopCommand;
	const parsed = DESKTOP_COMMAND_ARGS[name].safeParse(args);
	if (!parsed.success) {
		return {
			ok: false,
			error: `Desktop command '${name}' has invalid arguments: ${describeIssues(parsed.error)}.`,
		};
	}
	return {
		ok: true,
		value: { command: name, args: parsed.data } as ParsedInvoke,
	};
}

function describeIssues(error: z.ZodError): string {
	return error.issues
		.map((issue) => {
			const path = issue.path.length > 0 ? issue.path.join(".") : "(arguments)";
			if (issue.code === "unrecognized_keys") {
				return issue.keys
					.map((key) => (issue.path.length > 0 ? `${path}.${key}` : key))
					.map((key) => `${key} (not allowed)`)
					.join(", ");
			}
			return `${path} (${issue.code})`;
		})
		.join(", ");
}
