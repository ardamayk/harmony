#!/usr/bin/env bash
set -euo pipefail
command="${1:?Desktop command is required}"
shift
case "$command" in
  start)
    exec ./scripts/run-linux-webkit.sh "${CARGO_TARGET_DIR:-$PWD/src-tauri/target}/release/earthly-audio-desktop" "$@"
    ;;
  build) exec tauri build --no-bundle "$@" ;;
  sidecar|prepared)
    ./scripts/prepare-mpv-sidecar.sh
    configArgs=(--config src-tauri/tauri.sidecar.conf.json)
    if [[ "$command" == prepared ]]; then configArgs+=(--config '{"build":{"beforeBuildCommand":""}}'); fi
    exec tauri build --no-bundle "${configArgs[@]}" "$@"
    ;;
  dev)
    ./scripts/prepare-mpv-sidecar.sh
    exec ./scripts/run-linux-webkit.sh tauri dev --config src-tauri/tauri.sidecar.conf.json "$@"
    ;;
  *) echo "Unknown Desktop command: $command" >&2; exit 2 ;;
esac
