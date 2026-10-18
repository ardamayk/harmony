import type {
	ManagedImportAlbumDecision,
	ManagedImportAlbumPreview,
	ManagedImportBatch,
} from "@repo/api-client";
import type { ImportFileEntry } from "./-managed-import-workflow";

export function defaultAlbumDecision(
	album: ManagedImportAlbumPreview,
): ManagedImportAlbumDecision {
	const existing =
		album.existingAlbums.length === 1 ? album.existingAlbums[0] : undefined;
	return {
		albumKey: album.key,
		albumId: existing?.id,
		createSeparate: false,
		artworkMode: existing ? "none" : "auto",
	};
}

function importAlbumKey(entry: ImportFileEntry): string {
	return entry.preview?.file.albumKey ?? "";
}

export function importPositionKey(entry: ImportFileEntry): string {
	const file = entry.preview?.file;
	return file
		? JSON.stringify([importAlbumKey(entry), file.discNo, file.trackNo])
		: entry.key;
}

export function normalizeImportTitle(title: string): string {
	return title.normalize("NFC").trim().replace(/\s+/g, " ").toLowerCase();
}

export function reviewImportEntries(
	entries: ImportFileEntry[],
	batch: ManagedImportBatch | undefined,
	decisions: Record<string, ManagedImportAlbumDecision>,
) {
	const hashes = new Set<string>();
	return entries.map((entry) => {
		if (!entry.preview || entry.state !== "accepted") return entry;
		const hash = entry.preview.file.contentSha256;
		if (hash && hashes.has(hash))
			return {
				...entry,
				selected: false,
				errorMessage:
					"Identical file already selected in this import; this copy will be skipped.",
			};
		if (hash) hashes.add(hash);
		const album = batch?.albums?.find(
			(candidate) => candidate.key === importAlbumKey(entry),
		);
		if (!album) return entry;
		const decision = decisions[album.key] ?? defaultAlbumDecision(album);
		const target = decision.createSeparate
			? undefined
			: album.existingAlbums.find(
					(candidate) => candidate.id === decision.albumId,
				);
		const file = entry.preview.file;
		const matchingTracks =
			target?.tracks.filter(
				(track) =>
					track.discNo === file.discNo && track.trackNo === file.trackNo,
			) ?? [];
		return { ...entry, preview: { ...entry.preview, matchingTracks } };
	});
}

export function importReviewProblems(
	entries: ImportFileEntry[],
	albums: ManagedImportAlbumPreview[],
	decisions: Record<string, ManagedImportAlbumDecision>,
): string[] {
	const problems = new Set<string>();
	const positions = new Set<string>();
	const selectedAlbums = new Set<string>();
	for (const entry of entries) {
		if (!entry.selected || entry.state !== "accepted" || !entry.preview)
			continue;
		selectedAlbums.add(importAlbumKey(entry));
		const position = importPositionKey(entry);
		if (positions.has(position))
			problems.add("Choose one file for each Album position.");
		positions.add(position);
		const matches = entry.preview.matchingTracks ?? [];
		if (
			matches.some(
				(track) =>
					(track.titleKey ?? normalizeImportTitle(track.title)) !==
					(entry.preview?.file.titleKey ??
						normalizeImportTitle(entry.preview?.file.title ?? "")),
			)
		) {
			problems.add(
				"Another title occupies an Album position. Skip the file or create a separate Album.",
			);
		} else if (
			matches.length &&
			entry.duplicateDecision !== "replace_existing"
		) {
			problems.add("Confirm each Track Replacement or skip its file.");
		}
	}
	for (const album of albums) {
		if (!selectedAlbums.has(album.key)) continue;
		const decision = decisions[album.key] ?? defaultAlbumDecision(album);
		if (
			!decision.createSeparate &&
			!decision.albumId &&
			album.existingAlbums.length > 1
		)
			problems.add(`Choose the target Album for ${album.title}.`);
		const existing = decision.createSeparate
			? undefined
			: album.existingAlbums.find(
					(candidate) => candidate.id === decision.albumId,
				);
		if (existing?.hasArtwork) continue;
		if (decision.artworkMode === "auto" && album.artworks.length > 1)
			problems.add(
				`Choose a cover for ${album.title}, or continue without artwork.`,
			);
		if (
			decision.artworkMode === "selected" &&
			!album.artworks.some((option) => option.id === decision.artworkId)
		)
			problems.add(`Choose an available cover for ${album.title}.`);
	}
	return [...problems];
}
