#!/usr/bin/env bash
# Regenerates server-rs/.sqlx, the offline query data that lets server-rs
# build without a database. `--check` only verifies it is current.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
schema="$root/target/sqlx/schema.db"
python3 "$root/scripts/sqlx-schema.py" "$schema" >/dev/null
cd "$root/server-rs"
DATABASE_URL="sqlite://$schema" cargo sqlx prepare "$@" -- --all-targets
