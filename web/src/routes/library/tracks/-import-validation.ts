import type { ManagedImportBatchFile } from "@repo/api-client";

type ValidationIssue = NonNullable<ManagedImportBatchFile["issues"]>[number];

const FIELD_LABELS: Record<string, string> = {
	TITLE: "Title",
	ARTIST: "Artist",
	ALBUMARTIST: "Album artist",
	ALBUM: "Album",
	GENRE: "Genre",
	TRACKNUMBER: "Track number",
	TOTALTRACKS: "Track total",
	DISCNUMBER: "Disc number",
	TOTALDISCS: "Disc total",
	DATE: "Date",
	APEv2: "APEv2 tag",
};

export function formatImportIssues(
	issues: ValidationIssue[] | undefined,
): string | undefined {
	if (!issues?.length) return undefined;
	return issues.map(formatImportIssue).join("\n");
}

export function formatImportIssue(issue: ValidationIssue): string {
	switch (issue.code) {
		case "missing_artwork":
			return "Embedded front cover not found. Add an image marked as Front Cover.";
		case "invalid_artwork":
			return "Embedded artwork is invalid. Check the image format and front-cover designation.";
		case "audio_decode_failed":
			return "Audio could not be decoded completely. The audio data may be damaged or incomplete.";
		case "unsupported_format":
			return "The file format or audio codec is not supported.";
		case "file_read_failed":
			return "The audio file could not be read.";
		default: {
			const label = FIELD_LABELS[issue.field] ?? issue.field;
			return label ? `${label}: ${issue.reason}` : issue.reason;
		}
	}
}
