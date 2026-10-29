import type { RadioStationPatch } from "@repo/api-client";
import { z } from "zod";
import { parseStationTags } from "./radio-station-tags";

const required = z.string().trim().min(1);
const optional = z.string().trim();

/** A station card's quick edit: name and stream URL, both required. */
export const radioStationQuickEditSchema = z.object({
	name: required,
	streamUrl: required,
});

/**
 * The station page's metadata form, as typed, to the patch it saves. Text
 * is trimmed, tags are comma-separated, and an empty bitrate is 0.
 */
export const radioStationFormSchema = radioStationQuickEditSchema.extend({
	homepageUrl: optional,
	faviconUrl: optional,
	country: optional,
	language: optional,
	tags: z.string().transform(parseStationTags),
	codec: optional,
	bitrate: z
		.string()
		.trim()
		.transform((bitrate) => (bitrate ? Number.parseInt(bitrate, 10) : 0))
		.pipe(z.int().nonnegative()),
}) satisfies z.ZodType<RadioStationPatch>;

export type RadioStationForm = z.input<typeof radioStationFormSchema>;
