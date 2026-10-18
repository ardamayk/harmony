import type { RadioStation } from "@repo/api-client";

export function matchesLocalStationFilter(
	station: RadioStation,
	filter: string,
): boolean {
	const query = filter.trim().toLowerCase();
	if (!query) {
		return true;
	}

	const haystack = [
		station.name,
		station.country,
		station.language,
		...station.tags,
	]
		.filter(Boolean)
		.join(" ")
		.toLowerCase();

	return haystack.includes(query);
}
