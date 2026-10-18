# Managed Import Web journey (Playwright)

`web/e2e/managed-import.spec.ts` drives the critical Managed Import journey
through the rendered Web Client against a real Music Server (issue #54). It
runs inside the normal `mise run web:test:e2e` / Integration Gate `web-e2e`
job; no extra CI wiring is needed.

## What it covers

1. The Tracks page plus action opens the Import Music modal and uploads a
   mixed batch: two accepted files, one without a title, one without an
   embedded front cover. The Import Preview, live region, progress bars, the
   `role="alert"` confirmation error, and the final outcomes are asserted.
2. A second batch resolves an Exact Duplicate (identical bytes under a new
   filename) and a Possible Duplicate (same Album position, different bytes)
   with an explicit "Import separately" decision.
3. Explicit Track Replacement from the row context menu keeps the Track ID and
   swaps the managed bytes.
4. Committed Tracks appear on the Tracks and Albums pages; every stream is
   compared byte-for-byte (SHA-256) with the uploaded fixture.
5. Import History shows the terminal batch results and expands per-file
   details.
6. Permanent Track Deletion requires the destructive confirmation dialog;
   cancelling leaves the Track playable.
7. Keyboard access: Enter opens the modal, Tab stays trapped inside, Escape
   closes and returns focus to the opener, closing an uncommitted batch asks
   `window.confirm`, and accepting it deletes the batch and its staging files.

## Fixtures

`web/e2e/fixtures/managed-import-audio.ts` builds Strict Import Profile MP3
files in Node: an ID3v2.4 tag with every required identity frame plus an
embedded PNG front cover, followed by the same MPEG-2 Layer III frames as
`server/internal/testutil/mp3.go`. Options omit frames or artwork to produce
rejected files, and add `TXXX` frames to vary the full-file hash.

Every run uses a unique run identifier in Artist, Album, and Track titles, so
the persistent `server/data/e2e.db` can accumulate earlier results without
triggering duplicate classification.

## Server layout

`web/playwright.config.ts` starts the Music Server with
`MANAGED_STORAGE_PATH=./data/e2e-managed` (relative to `server/`). The spec
resolves the same directory to inspect `.staging`, so keep the two in sync.
