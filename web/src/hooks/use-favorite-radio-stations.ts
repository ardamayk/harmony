import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { apiClient } from "#/lib/api";
import { radioQueryKeys } from "#/lib/radio-query-keys";

/**
 * Favorite state of saved Radio Stations, read from the shared stations list
 * so the Player Bar and the Radio pages agree after a toggle.
 */
export function useFavoriteRadioStations() {
	const queryClient = useQueryClient();
	const stations = useQuery({
		queryKey: radioQueryKeys.stations,
		queryFn: () => apiClient.listRadioStations(),
	});

	const update = useMutation({
		mutationFn: ({
			stationId,
			isFavorite,
		}: {
			stationId: string;
			isFavorite: boolean;
		}) => apiClient.patchRadioStation(stationId, { isFavorite }),
		onSuccess: async (_data, vars) => {
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: radioQueryKeys.stations }),
				queryClient.invalidateQueries({
					queryKey: radioQueryKeys.detail(vars.stationId),
				}),
			]);
		},
	});

	const isStationFavorite = useCallback(
		(stationId: string) =>
			stations.data?.items.some(
				(station) => station.id === stationId && station.isFavorite,
			) ?? false,
		[stations.data?.items],
	);

	const toggleStationFavorite = useCallback(
		(stationId: string, isFavorite: boolean) => {
			update.mutate({ stationId, isFavorite });
		},
		[update],
	);

	return { isStationFavorite, toggleStationFavorite };
}
