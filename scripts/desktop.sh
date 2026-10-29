#!/usr/bin/env bash
# Runs a Desktop Client command for this machine's target (scripts/desktop-target.ts):
# the Windows Desktop Client from WSL (scripts/desktop-windows/cli.ts), the
# Linux one everywhere else (desktop/package.json, scripts/run-desktop.sh).
#   dev    debug core and Vite HMR against a running Music Server
#   build  the release client; `mise run desktop:build` builds web/dist first
#   start  the built client; --wait keeps a Windows one in the foreground
#   clean  the build output
# It never starts a Music Server: `mise run dev` and `mise run start` wrap it
# in scripts/run-with-music-server.sh.
set -euo pipefail

command="${1:?Desktop command is required: dev, build, start or clean}"
shift
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

waitFlag=""
# Node warns that the repository root sets no package type; that is noise here.
target="$(node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON scripts/desktop-target.ts)"
readonly serverHealthUrl="${EARTHLY_SERVER_HEALTH_URL:-http://${SERVER_ADDR:-127.0.0.1:8090}/api/v1/health}"
echo "Desktop Client target: $target (EARTHLY_DESKTOP_TARGET=linux|windows overrides it)."

# The client has a connection screen, so a missing Music Server is no error.
hintWithoutMusicServer() {
	curl --silent --fail --output /dev/null --max-time 1 "$serverHealthUrl" && return 0
	cat >&2 <<EOF
No Music Server answers at $serverHealthUrl.
Start one with 'mise run server:dev' or 'mise run server:start', or run both with 'mise run dev'.
The client starts anyway; connect it from its connection screen.
EOF
}

cleanLinux() {
	local targetDirectory="${CARGO_TARGET_DIR:-target}"
	rm -rf desktop/dist desktop/core/binaries
	rm -f "$targetDirectory/release/earthly-audio-core" "$targetDirectory/release/mpv" \
		"$targetDirectory/debug/earthly-audio-core"
}

case "$target:$command" in
	*:dev)
		hintWithoutMusicServer
		;;
	*:start)
		[[ "${1:-}" == --wait ]] && { waitFlag="--wait"; shift; }
		hintWithoutMusicServer
		;;
esac

case "$target:$command" in
	linux:dev | linux:build | linux:start)
		# The package scripts run scripts/run-desktop.sh.
		exec pnpm --dir desktop run "$command" "$@"
		;;
	linux:clean)
		cleanLinux
		;;
	windows:build)
		# web/dist comes from the task's web:build dependency.
		exec node scripts/desktop-windows/cli.ts build --web-built "$@"
		;;
	windows:start)
		exec node scripts/desktop-windows/cli.ts start ${waitFlag:+"$waitFlag"} "$@"
		;;
	windows:dev | windows:clean)
		exec node scripts/desktop-windows/cli.ts "$command" "$@"
		;;
	*)
		echo "Unknown Desktop command: $command (dev, build, start or clean)" >&2
		exit 2
		;;
esac
