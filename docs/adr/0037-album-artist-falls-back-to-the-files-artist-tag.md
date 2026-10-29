---
status: accepted
---

# Album Artist falls back to the file's Artist tag

[ADR 0019](0019-tag-based-import-and-album-artwork.md) requires Title, Artist, Album Artist, Album, and Track number, and [ADR 0011](0011-managed-import-and-bit-preserving-storage.md) rejects missing metadata instead of synthesizing it. Files from a [Soulseek Acquisition](0036-soulseek-acquisition-through-an-operator-run-slskd.md) commonly carry no ALBUMARTIST tag, so whole Albums were rejected although every file named its Artist.

When a file's Album Artist tag is missing or blank, Managed Import uses that same file's Artist credits as its Album Artist. The rule holds for every Managed Import, uploaded or acquired. The fallback value is the Album Artist everywhere: it forms the Album identity that groups files into Albums and it is the Album Artist stored for the Album. Nothing is inferred from file names or an online service, and the file bytes are never rewritten, so Bit-Preserving Storage stands. A file without an Artist tag is still rejected, since Artist is required on its own.

The Import Preview shows a notice on each file whose Album Artist came from its Artist tag.

## Considered Options

- **Keep rejecting**: keeps the tags authoritative but turns away most acquired Albums and gives the listener no way forward short of retagging outside the app.
- **Fall back only for Soulseek Acquisitions**: rejected to keep one Strict Import Profile; the same file should import the same way however it arrived.

## Consequences

- A Track whose Artist credit differs from the rest of its Album, such as a featuring credit, forms its own Album when the Album Artist tag is missing. The per-file notice makes that visible in the Import Preview before confirmation.
- A missing Artist is reported once, under Artist, rather than also under Album Artist.
