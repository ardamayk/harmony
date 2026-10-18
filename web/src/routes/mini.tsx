import { createFileRoute } from "@tanstack/react-router";
import { MiniPlayerPage } from "./-mini-page";

export const Route = createFileRoute("/mini")({
	component: MiniPlayerPage,
});
