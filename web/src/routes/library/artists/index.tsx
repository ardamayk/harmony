import { createFileRoute } from "@tanstack/react-router";
import { z } from "zod";
import { ArtistsRoute } from "./-artists-route";

const artistsSearchSchema = z.object({
	q: z.string().optional(),
});

export const Route = createFileRoute("/library/artists/")({
	validateSearch: artistsSearchSchema,
	component: ArtistsRoute,
});
