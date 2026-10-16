#!/usr/bin/env bash
# Copies the built Web Client and product docs into server/static, which the
# Music Server embeds at compile time.
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

case "$TARGET" in
  all)
    sync_web
    sync_docs
    ;;
  web)
    sync_web
    ;;
  docs)
    sync_docs
    ;;
  *)
    echo "usage: $0 [all|web|docs]" >&2
    exit 2
    ;;
esac
