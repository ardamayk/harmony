#!/usr/bin/env bash
# Generates the TypeScript API client types from the OpenAPI contract.
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_ROOT="$REPOSITORY_ROOT"

if [[ $# -gt 0 ]]; then
	if [[ $# -ne 2 || "$1" != "--output-root" ]]; then
		echo "Usage: $0 [--output-root PATH]" >&2
		exit 2
	fi
	OUTPUT_ROOT="$2"
fi

CONTRACT_ROOT="$REPOSITORY_ROOT/packages/contracts"
if [[ "$OUTPUT_ROOT" != "$REPOSITORY_ROOT" ]]; then
	CONTRACT_ROOT="$OUTPUT_ROOT/packages/contracts"
	mkdir -p "$CONTRACT_ROOT"
	cp "$REPOSITORY_ROOT/packages/contracts/openapi.yaml" "$CONTRACT_ROOT/openapi.yaml"
fi

mkdir -p "$OUTPUT_ROOT/packages/api-client/src/generated"

if [[ -n "${OPENAPI_TYPESCRIPT_BIN:-}" ]]; then
	"$OPENAPI_TYPESCRIPT_BIN" \
		"$CONTRACT_ROOT/openapi.yaml" \
		-o "$OUTPUT_ROOT/packages/api-client/src/generated/schema.ts"
else
	(cd "$REPOSITORY_ROOT/packages/contracts" && pnpm exec openapi-typescript \
		"$CONTRACT_ROOT/openapi.yaml" \
		-o "$OUTPUT_ROOT/packages/api-client/src/generated/schema.ts")
fi
