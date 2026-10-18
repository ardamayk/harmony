import { createFileRoute } from "@tanstack/react-router";
import { ArtistDetailPage } from "./-artist-page";

export const Route = createFileRoute("/library/artists/$artistId")({
	component: ArtistDetailPage,
});
