#!/usr/bin/env bash
# Starts the Music Server, runs the given client command in the foreground,
# and stops the Music Server again when the client exits or the script is
# interrupted. The script owns the Music Server it starts unless explicit
# reuse is requested.
set -euo pipefail

readonly SERVER_ADDRESS="${SERVER_ADDR:-127.0.0.1:8090}"
readonly SERVER_HEALTH_URL="${EARTHLY_SERVER_HEALTH_URL:-http://$SERVER_ADDRESS/api/v1/health}"
readonly REUSE_RUNNING_SERVER="${EARTHLY_REUSE_MUSIC_SERVER:-0}"
readonly SERVER_STARTUP_ATTEMPTS=100
readonly SERVER_STARTUP_DELAY_SECONDS=0.1
readonly SERVER_SHUTDOWN_ATTEMPTS=150
readonly SERVER_SHUTDOWN_DELAY_SECONDS=0.1

serverPid=""

isMusicServerReady() {
	local response
	response="$(curl --silent --show-error --max-time 1 "$SERVER_HEALTH_URL" 2>/dev/null)" || return 1
	[[ "$response" == *'"status":"ok"'* && "$response" == *'"api.v1"'* ]]
}

describeListeningProcesses() {
	local port="${SERVER_ADDRESS##*:}"
	ss -ltnp 2>/dev/null | awk -v port=":$port" '$4 ~ port"$" { print "  " $0 }'
}

serverProcessGroupAlive() {
	[[ -n "$serverPid" ]] && kill -0 -- "-$serverPid" 2>/dev/null
}

stopOwnedServer() {
	local exitCode=$?
	trap - EXIT INT TERM HUP
	if serverProcessGroupAlive; then
		echo "Stopping Music Server (process group $serverPid)."
		kill -TERM -- "-$serverPid" 2>/dev/null || true
		wait "$serverPid" 2>/dev/null || true
		local attempt
		for ((attempt = 1; attempt <= SERVER_SHUTDOWN_ATTEMPTS; attempt += 1)); do
			serverProcessGroupAlive || break
			sleep "$SERVER_SHUTDOWN_DELAY_SECONDS"
		done
		if serverProcessGroupAlive; then
			echo "Music Server did not stop in time; killing process group $serverPid." >&2
			kill -KILL -- "-$serverPid" 2>/dev/null || true
		fi
	fi
	exit "$exitCode"
}

startMusicServer() {
	local serverMode="$1"
	case "$serverMode" in
		development)
			setsid bash -c 'cd server && exec cargo run --quiet -p earthly-music-server' &
			;;
		built)
			setsid bash -c 'cd server && exec "${EARTHLY_SERVER_BINARY:-../bin/server}"' &
			;;
		*)
			echo "Unknown Music Server mode: $serverMode" >&2
			exit 2
			;;
	esac
	serverPid=$!
	trap stopOwnedServer EXIT INT TERM HUP
}

waitForMusicServer() {
	local attempt
	for ((attempt = 1; attempt <= SERVER_STARTUP_ATTEMPTS; attempt += 1)); do
		if isMusicServerReady; then
			return 0
		fi
		if ! kill -0 "$serverPid" 2>/dev/null; then
			wait "$serverPid" || true
			echo "Music Server stopped before becoming ready. Check its output and configured port." >&2
			return 1
		fi
		sleep "$SERVER_STARTUP_DELAY_SECONDS"
	done
	echo "Music Server did not become ready at $SERVER_HEALTH_URL." >&2
	return 1
}

refuseForeignMusicServer() {
	cat >&2 <<EOF
A Music Server is already listening at $SERVER_ADDRESS, but this script did not start it.
Stop it first so this task can own the Music Server it runs, or set EARTHLY_REUSE_MUSIC_SERVER=1
to attach to it (it will then keep running after the client exits).
Processes listening on that port:
EOF
	describeListeningProcesses >&2
	exit 1
}

if [[ $# -lt 2 ]]; then
	echo "Usage: $0 <development|built> <client command...>" >&2
	exit 2
fi

serverMode="$1"
shift

if isMusicServerReady; then
	if [[ "$REUSE_RUNNING_SERVER" == "1" ]]; then
		echo "Reusing Music Server at $SERVER_HEALTH_URL (EARTHLY_REUSE_MUSIC_SERVER=1); it will keep running after the client exits."
	else
		refuseForeignMusicServer
	fi
else
	startMusicServer "$serverMode"
	waitForMusicServer
fi

"$@"
