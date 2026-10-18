export function parseStationTags(value: string): string[] {
	return value
		.split(",")
		.map((tag) => tag.trim())
		.filter(Boolean);
}
