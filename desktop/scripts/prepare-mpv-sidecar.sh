#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIRECTORY="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP_DIRECTORY="$(cd -- "${SCRIPT_DIRECTORY}/.." && pwd)"
PINNED_MPV_VERSION="$(tr -d '\r\n' < "${DESKTOP_DIRECTORY}/src-tauri/mpv-version.txt")"
SOURCE_BINARY="${EARTHLY_AUDIO_MPV_PATH:-/usr/bin/mpv}"

if [[ ! -x "${SOURCE_BINARY}" ]]; then
  echo "Pinned mpv executable is missing at ${SOURCE_BINARY}." >&2
  exit 1
fi

VERSION_OUTPUT="$("${SOURCE_BINARY}" --version)"
VERSION_LINE="${VERSION_OUTPUT%%$'\n'*}"
if [[ "${VERSION_LINE}" != "mpv v${PINNED_MPV_VERSION}"* ]]; then
  echo "Desktop Client requires pinned mpv ${PINNED_MPV_VERSION}; found: ${VERSION_LINE}" >&2
  exit 1
fi

TARGET_TRIPLE="$(rustc --print host-tuple)"
if [[ "${TARGET_TRIPLE}" != *-unknown-linux-gnu ]]; then
  echo "Desktop mpv sidecar staging currently supports Linux GNU targets only." >&2
  exit 1
fi

BINARY_DIRECTORY="${DESKTOP_DIRECTORY}/src-tauri/binaries"
TARGET_BINARY="${BINARY_DIRECTORY}/mpv-${TARGET_TRIPLE}"
install -d -m 0755 "${BINARY_DIRECTORY}"
install -m 0755 "${SOURCE_BINARY}" "${TARGET_BINARY}"
ln -sfn "mpv-${TARGET_TRIPLE}" "${BINARY_DIRECTORY}/mpv"
echo "Staged pinned mpv ${PINNED_MPV_VERSION} for ${TARGET_TRIPLE}."
