import { createFileRoute } from "@tanstack/react-router";
import { SettingsPage } from "./-settings-page";

export const Route = createFileRoute("/settings/")({
	component: SettingsPage,
});
