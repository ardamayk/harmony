import { createFileRoute } from "@tanstack/react-router";
import { RadioStationDetailPage } from "./-station-page";

export const Route = createFileRoute("/radio/$stationId")({
	component: RadioStationDetailPage,
});
