/** React Query keys for Radio Stations, shared by the radio pages and the Player Bar. */
export const radioQueryKeys = {
	stations: ["radio", "stations"] as const,
	detail: (stationId: string) => ["radio", "stations", stationId] as const,
	nowPlaying: (stationId: string) =>
		["radio", "stations", stationId, "now-playing"] as const,
};
