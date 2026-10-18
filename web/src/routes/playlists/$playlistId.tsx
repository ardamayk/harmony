import { createFileRoute } from "@tanstack/react-router";
import { PlaylistDetailPage } from "./-playlist-page";

export const Route = createFileRoute("/playlists/$playlistId")({
	component: PlaylistDetailPage,
});
