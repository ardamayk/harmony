#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPORARY_ROOT="$(mktemp -d)"
trap 'rm -rf -- "$TEMPORARY_ROOT"' EXIT

bash "$REPOSITORY_ROOT/scripts/generate-openapi.sh" --output-root "$TEMPORARY_ROOT"

hasDrift=false
for generatedPath in \
	packages/api-client/src/generated/schema.ts; do
	committedFile="$REPOSITORY_ROOT/$generatedPath"
	generatedFile="$TEMPORARY_ROOT/$generatedPath"
	if ! cmp -s "$committedFile" "$generatedFile"; then
		hasDrift=true
		echo "Generated OpenAPI output is stale: $generatedPath" >&2
		diff -u \
			--label "committed/$generatedPath" \
			--label "generated/$generatedPath" \
			"$committedFile" "$generatedFile" || true
	fi
done

if [[ "$hasDrift" == true ]]; then
	echo "Run 'mise run generate' and commit the generated outputs." >&2
	exit 1
fi

echo "Committed OpenAPI outputs are current."
