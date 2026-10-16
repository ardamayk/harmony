#!/usr/bin/env bash
# Builds and launches the Electron Desktop Client (desktop/):
#   dev    debug Rust core, Vite dev server with HMR, and the shell
#   build  release Rust core (with the pinned mpv beside it) and the shell
#          bundle; `mise run desktop:build` builds web/dist first
#   start  the built release client, rendering web/dist
set -euo pipefail

command="${1:?Desktop command is required}"
shift
desktopDirectory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../desktop" && pwd)"
cd "$desktopDirectory"

readonly coreManifest="core/Cargo.toml"
readonly targetDirectory="${CARGO_TARGET_DIR:-$desktopDirectory/../target}"
readonly devServerUrl="http://localhost:3000"
readonly devServerStartupAttempts=300

launchElectron() {
	exec ./node_modules/.bin/electron . "$@"
}

case "$command" in
	dev)
		./scripts/prepare-mpv-sidecar.sh
		cargo build --manifest-path "$coreManifest"
		bun run build:shell
		# Its own process group, so stopping it also stops Vite under bun.
		setsid bun run --cwd ../web dev &
		webPid=$!
		trap 'kill -TERM -- "-$webPid" 2>/dev/null || true' EXIT INT TERM
		for ((attempt = 1; attempt <= devServerStartupAttempts; attempt += 1)); do
			curl --silent --output /dev/null "$devServerUrl" && break
			kill -0 "$webPid" 2>/dev/null || { echo "Vite dev server exited." >&2; exit 1; }
			sleep 0.1
		done
		EARTHLY_AUDIO_CORE_PATH="$targetDirectory/debug/earthly-audio-core" \
			EARTHLY_AUDIO_DEV_SERVER_URL="$devServerUrl" \
			./node_modules/.bin/electron . "$@"
		;;
	build)
		./scripts/prepare-mpv-sidecar.sh
		cargo build --release --manifest-path "$coreManifest"
		install -m 0755 "$(readlink -f core/binaries/mpv)" "$targetDirectory/release/mpv"
		bun run build:shell
		;;
	start)
		for required in "$targetDirectory/release/earthly-audio-core" dist/main.cjs ../web/dist/index.html; do
			[[ -e "$required" ]] || { echo "Missing $required; run 'mise run desktop:build' first." >&2; exit 1; }
		done
		EARTHLY_AUDIO_CORE_PATH="$targetDirectory/release/earthly-audio-core" launchElectron "$@"
		;;
	*)
		echo "Unknown Desktop command: $command" >&2
		exit 2
		;;
esac
