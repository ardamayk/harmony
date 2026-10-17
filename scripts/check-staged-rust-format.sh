#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGED_ROOT="$(mktemp -d)"
trap 'rm -rf -- "$STAGED_ROOT"' EXIT

git -C "$REPOSITORY_ROOT" checkout-index --all --prefix="$STAGED_ROOT/"

cargo fmt --manifest-path "$STAGED_ROOT/Cargo.toml" --all -- --check
