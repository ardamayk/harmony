---
status: accepted
---

# Managed Import uses file tags, hash duplicates, and album-level artwork

The design interview and implementation plan were approved. This decision replaces ADR 0017 and the mandatory Genre and embedded artwork requirements in ADR 0011; the byte-preservation guarantee remains unchanged.

Managed Import will use validated file tags without Recording Identification, MusicBrainz, AcoustID, or Chromaprint. Remove the integration from the server, API contracts and generated clients, Import and Settings interfaces, configuration, dependency reporting, and Docker setup. Keep FFmpeg for audio validation. This deliberately gives up automatic identification and metadata enrichment in favor of a local, predictable import workflow.

Exact Duplicate is exclusively a full-file SHA-256 match across the library. Different bytes matching an existing Album, Disc number, Track number, and normalized Title propose an explicit Track Replacement, regardless of format or Artist-credit differences. Show metadata changes before confirmation and preserve the Track identity and its library references. A different Title occupying the same Album position is an integrity conflict that blocks that file until resolved, not another duplicate category.

Album matching uses Album Artist credits and the full Album title with case and redundant whitespace normalized. Year and Track Artist credits do not determine Album identity. Preserve edition qualifiers: "Album X" and "Album X (Deluxe)" always remain separate. Users can explicitly create a separate Album for different editions with otherwise matching tags.

Required file tags are Title, Artist, Album Artist, Album, and Track number. Disc number is required for multi-disc Albums and defaults to one for single-disc Albums. Genre, year, and artwork are optional. Reject missing required metadata with a reason rather than synthesizing it from filenames.

Artwork belongs to the Album. For a new Album, use a single valid embedded cover when available; when covers differ, offer selection among them, a JPEG/PNG upload, or no cover. When no valid embedded cover exists, offer an optional JPEG/PNG upload. Invalid embedded artwork produces a warning rather than rejecting otherwise valid audio. Preserve an existing Album's cover during import; adding a cover to an existing coverless Album requires user confirmation.

The user has emptied the music library. Existing music migration, album merging, and further music deletion are outside this implementation's scope.

## Preview choices

A Track Replacement Candidate offers replacement or skipping the incoming file. Declining replacement preserves the existing Track; it does not create a second Track in the same Album position. A user who wants to retain both versions can explicitly create a separate Album in the preview.

Different files in one Import Batch that propose the same Album, Disc number, Track number, and normalized Title are presented as Import File Alternatives, even when the library is empty. The user chooses one file and the others are skipped. Show format, size, and available technical audio properties without automatically ranking quality. Repeated files with the same full-file hash need only one import; skip the other copies as exact duplicates. Transfer completion order must not decide which alternative is retained.
