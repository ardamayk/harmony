import { createFileRoute } from "@tanstack/react-router";
import { AlbumDetailPage } from "./-album-page";

export const Route = createFileRoute("/library/$albumId")({
	component: AlbumDetailPage,
});
