# Repair managed Artist credits

Use the repair command from the same server version as the database. Stop the Music Server first; keep it stopped through preview, review, backup, and apply. Do not modify managed files during this operation. The command cannot enforce an application-wide offline lock.

From `server/`, report only (default):

```sh
cargo run --release --bin repair-artist-credits -- --db /absolute/path/app.sqlite --managed-root /absolute/path/managed
```

Review the JSON `changes`, `skipped`, and `conflicts`. `changes` are proposed until `applied` is true. Obtain separate user approval before applying to a real library. This command has only been validated on temporary test libraries, not a user's database.

Explicit apply, with a **new** backup filename:

```sh
cargo run --release --bin repair-artist-credits -- --db /absolute/path/app.sqlite --managed-root /absolute/path/managed --apply --backup /absolute/path/app.before-credit-repair.sqlite
```

Apply repeats inspection; it does not accept an edited or saved JSON report. Before metadata writes it creates a consistent SQLite `VACUUM INTO` backup, including committed WAL contents, syncs it, checks integrity and foreign keys. An existing destination is never overwritten. A failed verification may leave a diagnostic backup file; use a new destination after resolving the failure. A later repair failure retains the verified backup. No changes means no backup or writes.

The command opens only an existing database, registers the application's SQLite Search functions, and never runs migrations. Pending import, replacement, or deletion journals block operation: recover them using the Music Server, stop it again, rerun preview.

Only authoritative managed sources passing root-boundary, regular-file, size, SHA-256, and full existing media-inspector validation are eligible. Missing, changed, hidden, unreadable, or invalid Tracks are skipped. Album credits require every member to pass inspection and agree; conflicting identities, unknown old identities, and inconsistent membership are reported without merge/split. Recognized explicit-edition suffixes remain intact. Safe Track repairs remain independent of Album conflicts.

Apply rechecks the database snapshot under a write transaction; changed IDs, revisions, source paths/hashes, visibility, credits, or Album membership reject the stale preview. SQL errors roll back the entire repair. Track and Album metadata revisions advance only for changed credits. Existing Search triggers invalidate cached results.

Audio bytes, paths (including old metadata slugs), source revisions, Track/Album IDs, Playlist/Queue references, artwork, titles, positions, genres, lyrics, ReplayGain, and waveforms remain untouched. Only unreferenced old Artists may be removed; primary Album Artist references also protect them. A second successful run proposes no changes.
