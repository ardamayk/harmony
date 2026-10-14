#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_ROOT="$REPOSITORY_ROOT"
OAPI_CODEGEN_VERSION="v2.4.1"

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
	cp "$REPOSITORY_ROOT/packages/contracts/oapi-codegen.yaml" "$CONTRACT_ROOT/oapi-codegen.yaml"
fi

mkdir -p \
	"$OUTPUT_ROOT/packages/api-client/src/generated" \
	"$OUTPUT_ROOT/server/internal/api/gen"

(
	cd "$OUTPUT_ROOT/server"
	GOWORK=off go run \
		"github.com/oapi-codegen/oapi-codegen/v2/cmd/oapi-codegen@$OAPI_CODEGEN_VERSION" \
		-config ../packages/contracts/oapi-codegen.yaml \
		../packages/contracts/openapi.yaml
)

if [[ -n "${OPENAPI_TYPESCRIPT_BIN:-}" ]]; then
	"$OPENAPI_TYPESCRIPT_BIN" \
		"$CONTRACT_ROOT/openapi.yaml" \
		-o "$OUTPUT_ROOT/packages/api-client/src/generated/schema.ts"
else
	(cd "$REPOSITORY_ROOT/packages/contracts" && bun x openapi-typescript \
		"$CONTRACT_ROOT/openapi.yaml" \
		-o "$OUTPUT_ROOT/packages/api-client/src/generated/schema.ts")
fi
