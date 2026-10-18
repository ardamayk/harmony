#!/usr/bin/env bash
set -euo pipefail

MPV_SOURCE_SHA256="${MPV_SOURCE_SHA256:-ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209}"
MPV_BUILD_FLAGS="${MPV_BUILD_FLAGS:---buildtype=release --auto-features=disabled -Dbuild-date=false -Dgl=disabled -Dlibmpv=false}"
MPV_OUTPUT_DIRECTORY="${MPV_OUTPUT_DIRECTORY:-${RUNNER_TEMP:?RUNNER_TEMP is required}/earthly-audio-mpv}"
MPV_BUILD_LOG="${MPV_BUILD_LOG:-${RUNNER_TEMP}/mpv-build.log}"
MPV_VERSION="$(tr -d '\r\n' < desktop/core/mpv-version.txt)"
MPV_ARCHIVE="${RUNNER_TEMP}/mpv-${MPV_VERSION}.tar.gz"
MPV_SOURCE_DIRECTORY="${RUNNER_TEMP}/mpv-${MPV_VERSION}"
read -r -a mpvBuildFlags <<< "${MPV_BUILD_FLAGS}"

mkdir -p "$(dirname "${MPV_BUILD_LOG}")"
curl --fail --location --retry 3 \
  "https://github.com/mpv-player/mpv/archive/refs/tags/v${MPV_VERSION}.tar.gz" \
  --output "${MPV_ARCHIVE}"
echo "${MPV_SOURCE_SHA256}  ${MPV_ARCHIVE}" | sha256sum --check
tar --extract --gzip --file "${MPV_ARCHIVE}" --directory "${RUNNER_TEMP}"
meson setup "${MPV_SOURCE_DIRECTORY}/build" "${MPV_SOURCE_DIRECTORY}" "${mpvBuildFlags[@]}" \
  2>&1 | tee "${MPV_BUILD_LOG}"
meson compile -C "${MPV_SOURCE_DIRECTORY}/build" ./mpv:executable \
  2>&1 | tee -a "${MPV_BUILD_LOG}"
install -d -m 0755 "${MPV_OUTPUT_DIRECTORY}"
install -m 0755 "${MPV_SOURCE_DIRECTORY}/build/mpv" "${MPV_OUTPUT_DIRECTORY}/mpv"
binarySha256="$(sha256sum "${MPV_OUTPUT_DIRECTORY}/mpv" | cut -d ' ' -f 1)"
(cd "${MPV_OUTPUT_DIRECTORY}" && sha256sum mpv > mpv.sha256)

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "binary_sha256=${binarySha256}" >> "${GITHUB_OUTPUT}"
fi
