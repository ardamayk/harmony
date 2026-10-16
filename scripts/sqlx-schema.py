#!/usr/bin/env python3
"""Builds the schema database sqlx checks server-rs queries against.

Applies the Up section of every goose migration in server/migrations to a
fresh SQLite file, then drops the Library Search triggers: they call
library_search_text(), which only the servers register, and SQLite compiles
triggers when it prepares a write. Dropping them changes no query's shape.

    python3 scripts/sqlx-schema.py target/sqlx/schema.db
"""

import json
import pathlib
import sqlite3
import sys

root = pathlib.Path(__file__).resolve().parent.parent
target = pathlib.Path(sys.argv[1])
target.parent.mkdir(parents=True, exist_ok=True)
for suffix in ("", "-wal", "-shm"):
    pathlib.Path(f"{target}{suffix}").unlink(missing_ok=True)

connection = sqlite3.connect(target, isolation_level=None)
# Stand-ins for the servers' functions; the backfills run over empty tables.
connection.create_function("library_search_text", 2, lambda text, kind: json.dumps({}))
connection.create_function("library_search_version", 0, lambda: 1)

for path in sorted((root / "server/migrations").glob("*.sql")):
    up, section = [], None
    for line in path.read_text().splitlines():
        annotation = line.strip()
        if annotation.startswith("-- +goose "):
            keyword = annotation.removeprefix("-- +goose ").strip().upper()
            section = {"UP": "up", "DOWN": "down"}.get(keyword, section)
            continue
        if section == "up":
            up.append(line)
    connection.executescript("\n".join(up))

for (name,) in connection.execute(
    "SELECT name FROM sqlite_master WHERE type = 'trigger' AND sql LIKE '%library_search_%'"
).fetchall():
    connection.execute(f'DROP TRIGGER "{name}"')
connection.close()
print(f"schema database: {target}")
