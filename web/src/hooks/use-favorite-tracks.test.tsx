import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useFavoriteTracks } from "./use-favorite-tracks";

const listPlaylists = vi.fn();
const getPlaylist = vi.fn();
const addPlaylistTrack = vi.fn();
const removePlaylistTrack = vi.fn();

vi.mock("#/lib/api", () => ({
	apiClient: {
		listPlaylists: (...args: unknown[]) => listPlaylists(...args),
		getPlaylist: (...args: unknown[]) => getPlaylist(...args),
		addPlaylistTrack: (...args: unknown[]) => addPlaylistTrack(...args),
		removePlaylistTrack: (...args: unknown[]) => removePlaylistTrack(...args),
	},
}));

const favoritesPlaylist = {
	id: "favorites-id",
	name: "Favorites",
	isDefault: true,
	trackCount: 1,
};

const track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	albumId: "album-1",
	durationMs: 120000,
	format: "flac",
};

function createWrapper() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return function Wrapper({ children }: { children: ReactNode }) {
		return (
			<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
		);
	};
}

describe("useFavoriteTracks", () => {
	beforeEach(() => {
		listPlaylists.mockReset();
		getPlaylist.mockReset();
		addPlaylistTrack.mockReset();
		removePlaylistTrack.mockReset();

		listPlaylists.mockResolvedValue({
			items: [favoritesPlaylist],
			total: 1,
		});
		getPlaylist.mockResolvedValue({
			...favoritesPlaylist,
			tracks: [track],
		});
		addPlaylistTrack.mockResolvedValue({
			...favoritesPlaylist,
			trackCount: 2,
			tracks: [track, { ...track, id: "track-2", title: "Track 2" }],
		});
		removePlaylistTrack.mockResolvedValue({
			...favoritesPlaylist,
			trackCount: 0,
			tracks: [],
		});
	});

	it("reports favorite membership from the default Favorites playlist", async () => {
		const { result } = renderHook(() => useFavoriteTracks(), {
			wrapper: createWrapper(),
		});

		await waitFor(() => {
			expect(result.current.isFavorite("track-1")).toBe(true);
		});
		expect(result.current.isFavorite("track-2")).toBe(false);
	});

	it("adds favorites through the playlist API", async () => {
		getPlaylist.mockResolvedValue({
			...favoritesPlaylist,
			tracks: [],
		});

		const { result } = renderHook(() => useFavoriteTracks(), {
			wrapper: createWrapper(),
		});

		await waitFor(() => {
			expect(listPlaylists).toHaveBeenCalled();
			expect(getPlaylist).toHaveBeenCalled();
			expect(result.current.isFavorite("track-1")).toBe(false);
		});

		await act(async () => {
			result.current.toggleFavorite("track-1");
		});

		await waitFor(() => {
			expect(addPlaylistTrack).toHaveBeenCalledWith("favorites-id", "track-1");
		});
	});

	it("removes favorites through the playlist API", async () => {
		const { result } = renderHook(() => useFavoriteTracks(), {
			wrapper: createWrapper(),
		});

		await waitFor(() => {
			expect(result.current.isFavorite("track-1")).toBe(true);
		});

		await act(async () => {
			result.current.toggleFavorite("track-1");
		});

		await waitFor(() => {
			expect(removePlaylistTrack).toHaveBeenCalledWith(
				"favorites-id",
				"track-1",
			);
		});
	});
});
