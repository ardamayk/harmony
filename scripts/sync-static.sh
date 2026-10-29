#!/usr/bin/env bash
# Copies the built Web Client, product docs and Scalar's API reference bundle
# (@scalar/api-reference, pinned in packages/docs) into server/static, which
# the Music Server embeds at compile time.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-all}"

sync_web() {
  mkdir -p "$ROOT/server/static/web"
  rm -rf "$ROOT/server/static/web/"*
  cp -r "$ROOT/web/dist/." "$ROOT/server/static/web/"
}

sync_docs() {
  mkdir -p "$ROOT/server/static/docs"
  rm -rf "$ROOT/server/static/docs/"*
  cp -r "$ROOT/packages/docs/dist/." "$ROOT/server/static/docs/"
}

# Scalar's prebuilt standalone bundle, served at /api/docs/scalar/ so the API
# reference loads nothing from a CDN (ADR 0035). Needs `pnpm install`.
sync_api_reference() {
  local bundle="$ROOT/packages/docs/node_modules/@scalar/api-reference/dist/browser/standalone.js"
  if [[ ! -f "$bundle" ]]; then
    echo "Missing $bundle; run 'pnpm install' first." >&2
    exit 1
  fi
  mkdir -p "$ROOT/server/static/api-reference"
  rm -rf "$ROOT/server/static/api-reference/"*
  cp "$bundle" "$ROOT/server/static/api-reference/standalone.js"
}

case "$TARGET" in
  all)
    sync_web
    sync_docs
    sync_api_reference
    ;;
  web)
    sync_web
    ;;
  docs)
    sync_docs
    ;;
  api-reference)
    sync_api_reference
    ;;
  *)
    echo "usage: $0 [all|web|docs|api-reference]" >&2
    exit 2
    ;;
esac
