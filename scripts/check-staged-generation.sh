#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGED_ROOT="$(mktemp -d)"
trap 'rm -rf -- "$STAGED_ROOT"' EXIT

git -C "$REPOSITORY_ROOT" checkout-index --all --prefix="$STAGED_ROOT/"
# The staged zod generator resolves @hey-api/openapi-ts from the checkout.
ln -s "$REPOSITORY_ROOT/packages/contracts/node_modules" \
	"$STAGED_ROOT/packages/contracts/node_modules"

# Resolve the repository's pinned toolchain; the temporary checkout is not a
# trusted mise directory and would otherwise fall back to global tool versions.
mise exec -C "$REPOSITORY_ROOT" -- bash -c '
	cd "$1"
	OPENAPI_TYPESCRIPT_BIN="$2/packages/contracts/node_modules/.bin/openapi-typescript" \
		bash scripts/check-openapi-generation.sh
' _ "$STAGED_ROOT" "$REPOSITORY_ROOT"
