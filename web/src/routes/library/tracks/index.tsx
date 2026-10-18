import { createFileRoute } from "@tanstack/react-router";
import { z } from "zod";
import { TracksPage } from "./-tracks-page";

export const Route = createFileRoute("/library/tracks/")({
	validateSearch: z.object({
		artistId: z.string().optional(),
		q: z.string().optional(),
	}),
	component: TracksRoute,
});

function TracksRoute() {
	const { artistId, q } = Route.useSearch();
	const navigate = Route.useNavigate();
	return (
		<TracksPage
			artistId={artistId}
			search={q}
			onClearFilters={() => void navigate({ search: {} })}
		/>
	);
}
