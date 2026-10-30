import { createFileRoute } from "@tanstack/react-router";
import { ListenTogetherPage } from "./-listen-together-page";

export const Route = createFileRoute("/listen-together/")({
	component: ListenTogetherPage,
});
