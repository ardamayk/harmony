---
status: accepted
---

# Managed Import retries remain within the running client

The agreed recovery scope is the current running Playback Client: reopening the application does not restore an unfinished Import Batch. An interrupted audio-file transfer restarts from the beginning rather than resuming at a byte offset. This keeps recovery within the existing whole-file transfer model and avoids persistent client file-access recovery and a chunked upload protocol.

For connection failures, each affected file receives three automatic retries after waits of 2, 5, and 10 seconds, in addition to its initial attempt. When those retries are exhausted, the Import Preview offers a bulk retry of affected files or confirmation of the ready files.

Each explicit bulk retry starts a new cycle: one immediate attempt followed by up to three automatic retries. Only files with transient transfer failures participate; validation failures and size-limit rejections do not. An exhausted cycle never restarts without user action.

Confirming the ready files finalizes the Import Batch. Unsuccessful files remain visible in the results with their reasons, but importing them subsequently requires a new Managed Import. Before confirmation, the client states how many unsuccessful files will not be imported.

Dialog dismissal is phase-dependent. During upload, validation, or committing, the close button and outside clicks minimize the dialog to a compact status indicator while work continues. When an Import Preview is ready and awaiting confirmation, those actions request cancellation confirmation before discarding staged files. Dismissing the results screen simply closes it. Selecting files still starts upload and validation automatically; no separate start button is introduced. Application exit ends the client session without restoring it on relaunch.

Explicit cancellation stops active transfers and pending retries and removes all uncommitted staged files in the batch, including files already ready for confirmation. The client asks for confirmation before discarding this work. Cancellation is unavailable once committing starts and remains unavailable until that operation finishes.

Both Web and Desktop Clients process at most two files concurrently. A slot covers transfer and validation through the preview response. Retry delays release their slots so queued files can progress. No user-facing concurrency setting is introduced; subsequent tuning depends on measured behavior.

The upload route replaces the current total request-read deadline with a 30-second byte-inactivity deadline, renewed as bytes arrive. A progressing transfer is not interrupted solely because of its total duration; byte and disk-capacity limits still apply. Validation after transfer does not use this transfer-inactivity deadline.

Before retrying a transfer whose response was lost, the client checks the existing job's state to avoid retransmitting an already accepted file. Disk-capacity and validation failures require user action rather than automatic transfer retries.

Progress presentation leads with ready, processing, and queued file counts. A file's percentage describes byte transfer only, accompanied by transferred and total bytes. Validation and readiness use distinct named phases. Retry waits show a countdown and the retry number within the current cycle. Speed, elapsed time, and technical error details belong in expandable details. Committing has its own completed-file count rather than reusing the transfer percentage.

Each Playback Client keeps at most one open Import Batch. The existing import action reopens that batch, including when minimized, instead of creating another. A new batch can start after the previous one finishes or is canceled. Separate clients may have separate batches.

The client sends periodic liveness signals while an unfinished batch exists, including while awaiting confirmation. Staged files remain available while the client stays connected. A minimized operation that becomes ready changes its indicator to Ready for review without opening the dialog automatically. Application exit requests cleanup; if exit cleanup cannot reach the server, uncommitted staging becomes eligible for cleanup 15 minutes after the last client liveness signal. Committing files remain subject to commit recovery rather than unsafe staging deletion.

The design decisions above were approved for implementation. The implementation uses the HTTP API, import UI, and Desktop bridge as its verification boundaries.
