# Soulseek Acquisition

Decision record: [ADR 0036](../../adr/0036-soulseek-acquisition-through-an-operator-run-slskd.md). Terms: Soulseek Acquisition, Acquisition Version, Acquisition Source and Soulseek Sharing in `CONTEXT.md`.

## Problem Statement

A listener finds an External Track, plays it in full and decides they want it. The External Source never stores anything (ADR 0032), so keeping it means finding a file somewhere else, downloading it by hand, checking its quality and running a Managed Import. For a whole Album in lossless quality that is slow and error-prone, and the listener has no way to see which qualities exist before downloading.

## Solution

When the operator runs slskd and configures the Music Server for it, an External Track offers **Install**. The listener chooses the Track alone or its whole Album, may edit the query, and sees every quality the Soulseek network offers, grouped into Acquisition Versions (for example "FLAC 24-bit / 48 kHz, 19 files, 7 sources"). They choose a version, typically after checking the release's quality elsewhere. The Music Server downloads it through slskd, trying another source of the same version when one fails, and hands the files to Managed Import. The listener reviews the Import Preview and confirms, exactly as for an upload. The settings show whether the library is shared on Soulseek; the operator decides that.

## User Stories

1. As an operator, I want Soulseek Acquisition off unless I configure slskd, so that my server never touches Soulseek without my decision.
2. As an operator, I want to point the Music Server at my own slskd with its address and an API key, so that I keep control of the Soulseek client and account.
3. As an operator, I want to tell the Music Server where slskd writes downloads, so that it can import them.
4. As an operator, I want the server to refuse to start the feature with a clear error when slskd's download directory is missing or unreadable, so that I find configuration mistakes early.
5. As an operator, I want the server to advertise the feature only when it is configured, so that clients hide Install otherwise.
6. As an operator, I want every Soulseek route to answer 404 when the feature is off, so that the API behaves like the External Source.
7. As an operator, I want the README and settings to state that I am responsible for what is downloaded and shared, so that the legal position is clear.
8. As an operator, I want the project never to ship or install slskd, so that the project does not distribute a peer-to-peer client.
9. As an operator, I want to share Managed Storage by configuring slskd myself, so that sharing stays off unless I choose it.
10. As a listener, I want to see in the settings whether my library is shared on Soulseek, so that I know what other users can see.
11. As a listener, I want the settings to show whether slskd is reachable and logged in to Soulseek, so that I understand why Install might fail.
12. As a listener, I want an Install action on an External Track in search results, so that I can keep a song I just discovered.
13. As a listener, I want the same Install action in the Player Bar while an External Track plays, so that I can keep it while listening.
14. As a listener, I want to choose between the Track alone and its whole Album, with the Album as the default, so that I get the release rather than a single file.
15. As a listener, I want the Soulseek query prefilled from the External Track's artist and title or Album, so that I rarely type anything.
16. As a listener, I want to edit the query before searching, so that a messy External Track title such as "(Official Video)" does not spoil the search.
17. As a listener, I want results to appear while the search is still running, so that I do not wait for the whole network.
18. As a listener, I want results grouped into Acquisition Versions by format, bit depth and sample rate, so that I choose a quality rather than a stranger's folder.
19. As a listener, I want lossy versions grouped by format and bitrate, so that "MP3 320" and "MP3 V0" are distinct choices.
20. As a listener, I want a version whose sources do not report bit depth or sample rate shown as "unknown" rather than guessed, so that I am not misled.
21. As a listener, I want each version to show its file count, total size and how many sources offer it, so that I can judge its availability.
22. As a listener, I want incomplete Album sources marked with their file count, such as "12/19", and listed below complete ones, so that I do not install half an Album by mistake.
23. As a listener, I want to open a version and see its sources with their free upload slot, queue length and speed, so that I can pick a specific source when I want to.
24. As a listener, I want no version marked as "best" or "original", so that the choice of quality is mine.
25. As a listener, I want to start an acquisition by choosing a version, so that the server picks a good source for me.
26. As a listener, I want the server to move to the next source of the same version when a source fails or refuses, so that one unreliable user does not stop the acquisition.
27. As a listener, I want an acquisition to fail with a clear reason when no source of the version is left, so that I can choose another version.
28. As a listener, I want an acquisition in progress to appear on the Import page as an Import Batch, so that downloads and imports live in one place.
29. As a listener, I want to see per-file progress, the current source and how many sources were tried, so that I know what is happening.
30. As a listener, I want to cancel an acquisition, so that I can stop a download I no longer want.
31. As a listener, I want an acquisition to keep going when I close the browser or the Desktop Client, so that a long Album download does not depend on my session.
32. As a listener, I want an acquisition to survive a Music Server restart, so that a restart does not lose progress slskd already made.
33. As a listener, I want downloaded files to go through the Strict Import Profile and an Import Preview, so that badly tagged or broken files never enter the library unseen.
34. As a listener, I want rejected files listed with their reasons in the Import Preview, so that I can decide to try another version.
35. As a listener, I want to confirm the ready files as for any Managed Import, so that I keep the final say over my library.
36. As a listener, I want the source folder's cover image used as Album Artwork when present, so that the Album looks complete.
37. As a listener, I want Exact Duplicates and Track Replacement Candidates handled as in any Managed Import, so that an acquisition never silently duplicates or replaces a Track.
38. As a listener, I want the Import Preview to show the format, bit depth and sample rate the files actually have, so that I can compare them with what the version claimed.
39. As a listener, I want the downloaded copies removed after I confirm, reject or cancel, so that my disk holds one copy in Managed Storage.
40. As a listener, I want an acquisition awaiting review to wait for me without expiring, so that I can review it hours later.
41. As a listener, I want the Web and Desktop Clients to behave the same, so that Install works wherever I listen.
42. As a listener, I want Install hidden for an External Track when slskd is not configured, so that I never see an action that cannot work.
43. As a listener, I want a clear message when slskd is unreachable or not logged in when I press Install, so that I know the problem is not mine.
44. As a listener, I want starting a second acquisition of the same version while one is running to reopen the running one, so that I do not download twice.

## Implementation Decisions

### Configuration and capability

- Three operator settings, environment variables like the other optional services: slskd's base URL, an slskd API key, and the path where slskd's completed downloads appear to the Music Server. All three set means on; none set means off; a partial configuration fails startup with an actionable message.
- The API key needs slskd's `ReadWrite` role (searches, transfers, share rescan). The Soulseek account is slskd's own configuration; the Music Server never sees its password.
- Capability `soulseek-acquisition.v1`, advertised only when configured. With it absent, every Soulseek route answers 404 `soulseek_acquisition_disabled`, mirroring `external_source_disabled`.
- The server's dependency report (Server Dependency) gains slskd: reachable, Soulseek connection state, logged-in user name, slskd version.

### New `soulseek` server module

Layered like every module (wiring, handlers, service, repository) plus an slskd HTTP client submodule. It reaches Managed Import only through Managed Import's public service API.

- **slskd client**: a deep module that hides slskd's REST shapes. Its interface: start a search, read a search's responses, enqueue downloads for one user's files, read transfer states, remove finished transfers, read the server and session state, list shares. Every call has a timeout; slskd errors map to one error type with a reason the clients can show.
- **Version grouping** is a pure function from slskd search responses to Acquisition Versions, so it is unit-testable without HTTP:
  - Only audio files with a supported Managed Import extension count; cover images in the same folder are remembered as artwork candidates.
  - An Acquisition Source is one (user, folder). For Track scope the source is one file.
  - The version key is the format plus, for lossless, bit depth and sample rate, and for lossy, bitrate and whether it is variable. Missing attributes form their own "unknown" key; nothing is inferred.
  - A source whose files disagree on the key is split per key, never merged.
  - For Album scope a source's completeness is its file count against the largest file count in its version. Versions sort by complete-source count, then source count; sources sort complete first, then free slot, then queue length, then speed. This is an ordering for display, not a quality ranking: versions of different quality are never compared.
- **Searches** are not persisted: the server starts the slskd search and reads it back by slskd's search id. A search lives as long as slskd keeps it.
- **Acquisitions are persisted** in a new table (migration), keyed by user: query, scope, version key, the ordered source list snapshotted at start, the current source index, state, the Managed Import batch id, failure reason, timestamps. A child table holds each remote file: its source, slskd transfer id, state, bytes, the Managed Import job id and its outcome.
- **Acquisition states**: `downloading`, `importing`, `review`, `completed`, `failed`, `cancelled`. `review` is the Import Preview awaiting confirmation; `completed` follows confirmation.
- **A background worker** polls slskd for every active acquisition every few seconds. It is the only writer of transfer state. On a failed, rejected or stalled source it removes that source's transfers and enqueues the next source's missing files. A file that already arrived from an earlier source is kept. Stalled means no progress for a fixed interval.
- **Restart recovery**: on startup the worker reloads active acquisitions and reconciles them with slskd's transfer list, then continues.
- **Duplicate start**: starting an acquisition for the same query, scope and version key while one is active returns the active one.
- Multi-statement writes run in one `db::begin_write` transaction.

### Managed Import changes

- Managed Import gains a **server-fed** entry point: create a batch on behalf of an acquisition, and add one file to a job from a local path rather than an HTTP body. It goes through the same staging, SHA-256, Strict Import Profile and Import Preview path as an upload; only the byte source differs.
- An acquisition's batch is **server-owned**: it is exempt from client-liveness cleanup and from the client's single-open-batch rule in ADR 0018, and it lives until confirmed or cancelled. ADR 0036 records this exception to ADR 0018.
- A source folder's cover image, when present and accepted, is attached through the existing album artwork upload.
- Confirmation, Exact Duplicate, Track Replacement Candidate, Import File Alternatives and Album Position Conflict behave unchanged; the batch is confirmed with the existing confirm route.
- After a file is staged, its copy in the download directory is deleted and its slskd transfer removed. On cancellation, partial and finished downloads are deleted too.

### API contract (OpenAPI first, then generated client)

- `GET /api/v1/soulseek/status`: slskd reachability, Soulseek connection and login state, and the sharing state of Managed Storage (`shared`, `not_shared` or `unknown`) with the shared file count.
- `POST /api/v1/soulseek/searches` with query and scope (`album` or `track`): returns a search id.
- `GET /api/v1/soulseek/searches/{searchId}`: search state (`searching` or `complete`) and the current Acquisition Versions with their sources. Clients poll it while searching.
- `POST /api/v1/soulseek/acquisitions` with search id, version key and optionally one source id: starts or returns the acquisition.
- `GET /api/v1/soulseek/acquisitions` and `GET /api/v1/soulseek/acquisitions/{acquisitionId}`: state, current source, sources tried, per-file progress, the Managed Import batch id and failure reason.
- `DELETE /api/v1/soulseek/acquisitions/{acquisitionId}`: cancels; unavailable while committing, as for any Import Batch.
- Responses follow `gojson` encoding rules (ADR 0025). Errors carry stable codes: `soulseek_acquisition_disabled`, `slskd_unreachable`, `soulseek_not_logged_in`, `search_not_found`, `version_not_found`, `acquisition_not_found`.

### Soulseek Sharing

- Read-only. The status route compares slskd's shares with Managed Storage's path as slskd sees it. The settings show the state and, when not shared, one line on how the operator shares it. There is no switch.

### Clients

- An **Install** action on External Track rows in the search dialog and in the Player Bar while an External Track plays, shown only with `soulseek-acquisition.v1`.
- An **Install dialog**: scope choice (Album default), editable query, a live list of Acquisition Versions, an expandable source list per version, and a start action. The dialog never highlights a version.
- The **Import page** shows acquisitions alongside Import Batches: downloading, then importing, then the existing Import Preview for review. Closing the dialog never cancels an acquisition.
- **Settings** gain a Soulseek section: connection, login, sharing state and the operator's responsibility note.
- All UI lives in the shared web code, so the Desktop Client gets it unchanged; any desktop-only difference is a bug. UI follows `docs/design-rules.md` and the `ui-design-new` skill.

## Testing Decisions

- A good test drives the feature from outside and asserts what a client or the library observes: HTTP responses, Import Preview contents, library Tracks, files on disk. It does not assert internal calls, table rows or worker timing.
- **Main seam: the assembled router against a fake slskd.** A small axum server in the test plays slskd: it serves scripted search responses and transfer states and writes files into a temporary download directory. Prior art: `server/tests/external_source.rs`, which drives the router against a fake yt-dlp. These tests cover:
  - disabled feature answers 404 and is not advertised; partial configuration fails startup;
  - search returns versions grouped and ordered as above;
  - an acquisition downloads, imports into an Import Preview and, after confirmation, appears in the library with the download copies gone;
  - a failing source falls over to the next source of the same version; exhausted sources fail the acquisition with the files that arrived;
  - cancellation removes staged and downloaded files;
  - restart recovery continues an acquisition from slskd's transfer state;
  - badly tagged files are rejected in the preview;
  - a server-owned batch survives client-liveness cleanup;
  - sharing state reads `shared` and `not_shared` correctly.
- **Pure unit tests** for version grouping, next to the module, with recorded slskd response fixtures (prior art: Managed Import's `service_tests.rs`).
- **Managed Import** server-fed entry point: covered through the main seam, plus one test in Managed Import's own tests that a server-fed file and an uploaded file with the same bytes produce the same preview.
- **Contract**: the existing contract test (`server/tests/contract.rs`) and generation check cover the new routes.
- **Web**: Vitest component tests for the Install dialog and the Import page's acquisition states against a mocked API client, following the existing web tests; one Playwright journey in `web/e2e` with the Soulseek routes stubbed at the network layer (prior art: `managed-import.spec.ts`).
- **Desktop**: checked by hand in the Desktop Client, as the Web and Desktop Clients behave alike.

## Out of Scope

- Any Qobuz, TIDAL or other store lookup of a release's quality, and any download from a streaming service.
- Automatic choice or quality ranking of versions.
- A sharing switch in the settings, or any change to slskd's configuration from the Music Server.
- Fake-quality detection (upsampled or transcoded files).
- Tag repair or a looser import profile for Soulseek files.
- Searching Soulseek without an External Track as the starting point.
- Shipping, installing or updating slskd, or a published image that contains it.
- Multi-user separation of acquisitions beyond keying rows by the default user.

## Further Notes

### Plan: slices, done in order in one pass

No issues are opened; this plan is the tracker. Each slice ends green (`mise run` checks, tests) and in its own commit.

1. **Docs and configuration.** Correct ADR 0036 (the Soulseek account is slskd's; server-owned batches are an exception to ADR 0018). Add the three settings, startup validation, the capability and the disabled 404 behavior, with the "disabled" and "partial configuration" tests.
2. **Contract.** All Soulseek routes and schemas in `openapi.yaml`; `mise run generate` for the client and offline query data.
3. **slskd client and status.** The client module, the fake slskd test harness, `GET /soulseek/status` including sharing state, and slskd in the dependency report.
4. **Search and versions.** Version grouping as a pure function with fixture tests; the search routes through the router.
5. **Acquisition lifecycle.** Migration for acquisitions and files, repository, worker with source failover, duplicate start, cancellation, restart recovery.
6. **Managed Import feed.** The server-fed entry point and server-owned batch, artwork from the source folder, download cleanup, and the end-to-end router test from search to library Track.
7. **Web UI.** Install action in search and the Player Bar, the Install dialog, acquisitions on the Import page, the Soulseek settings section; Vitest tests and the Playwright journey.
8. **Desktop parity and operator docs.** Check in the Desktop Client; README section on running slskd (read-only share of Managed Storage, shared download directory, API key role) and the responsibility note.
