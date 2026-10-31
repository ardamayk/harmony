#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIRECTORY="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP_DIRECTORY="$(cd -- "${SCRIPT_DIRECTORY}/.." && pwd)"
MINIMUM_MPV_VERSION="$(tr -d '\r\n' < "${DESKTOP_DIRECTORY}/core/mpv-minimum-version.txt")"
SOURCE_BINARY="${EARTHLY_AUDIO_MPV_PATH:-/usr/bin/mpv}"

if [[ ! -x "${SOURCE_BINARY}" ]]; then
  echo "mpv executable is missing at ${SOURCE_BINARY}." >&2
  exit 1
fi

VERSION_OUTPUT="$("${SOURCE_BINARY}" --version)"
VERSION_LINE="${VERSION_OUTPUT%%$'\n'*}"
FOUND_VERSION="$(sed -E 's/^mpv v?([0-9]+\.[0-9]+\.[0-9]+).*/\1/' <<< "${VERSION_LINE}")"
# A line without a release number is a git build, newer than any release.
if [[ "${FOUND_VERSION}" =~ ^[0-9] ]] &&
  [[ "$(printf '%s\n' "${MINIMUM_MPV_VERSION}" "${FOUND_VERSION}" | sort -V | head -n 1)" != "${MINIMUM_MPV_VERSION}" ]]; then
  echo "Desktop Client requires mpv ${MINIMUM_MPV_VERSION} or newer; found: ${VERSION_LINE}" >&2
  exit 1
fi

TARGET_TRIPLE="$(rustc --print host-tuple)"
if [[ "${TARGET_TRIPLE}" != *-unknown-linux-gnu ]]; then
  echo "Desktop mpv sidecar staging currently supports Linux GNU targets only." >&2
  exit 1
fi

BINARY_DIRECTORY="${DESKTOP_DIRECTORY}/core/binaries"
TARGET_BINARY="${BINARY_DIRECTORY}/mpv-${TARGET_TRIPLE}"
install -d -m 0755 "${BINARY_DIRECTORY}"
install -m 0755 "${SOURCE_BINARY}" "${TARGET_BINARY}"
ln -sfn "mpv-${TARGET_TRIPLE}" "${BINARY_DIRECTORY}/mpv"
echo "Staged ${VERSION_LINE%% Copyright*} for ${TARGET_TRIPLE}."
