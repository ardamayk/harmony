import { join, resolve } from "node:path";

export type ShellPaths = {
	/** The Rust core binary. */
	core: string;
	/** The built web UI served at app://earthly. */
	rendererRoot: string;
	preload: string;
	icon: string;
};

/**
 * Where the shell finds its parts. `desktopDirectory` is `desktop/` (the
 * bundled main script lives in `desktop/dist`). Development scripts point
 * at a debug core through EARTHLY_AUDIO_CORE_PATH.
 */
export function shellPaths(
	desktopDirectory: string,
	env: NodeJS.ProcessEnv,
): ShellPaths {
	return {
		core: resolve(
			env.EARTHLY_AUDIO_CORE_PATH ??
				join(desktopDirectory, "core/target/release/earthly-audio-core"),
		),
		rendererRoot: resolve(
			env.EARTHLY_AUDIO_RENDERER_DIR ?? join(desktopDirectory, "../web/dist"),
		),
		preload: join(desktopDirectory, "dist/preload.cjs"),
		icon: join(desktopDirectory, "resources/icon.png"),
	};
}

/**
 * The Vite dev server origin when EARTHLY_AUDIO_DEV_SERVER_URL is set;
 * `null` loads the built renderer. Only loopback origins are accepted.
 */
export function devServerOrigin(env: NodeJS.ProcessEnv): string | null {
	const raw = env.EARTHLY_AUDIO_DEV_SERVER_URL;
	if (!raw) return null;
	const url = new URL(raw);
	if (
		url.protocol !== "http:" ||
		!["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
	) {
		throw new Error(
			`EARTHLY_AUDIO_DEV_SERVER_URL must be a loopback http origin, got ${raw}`,
		);
	}
	return url.origin;
}
