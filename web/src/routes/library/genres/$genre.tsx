import { createFileRoute } from "@tanstack/react-router";
import { GenreDetailPage } from "./-genre-page";

export const Route = createFileRoute("/library/genres/$genre")({
	component: GenreDetailPage,
});
