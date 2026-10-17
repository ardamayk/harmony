import type { Album, Artist } from "@repo/api-client";
import { SearchField } from "#/components/search-field";
import { X } from "lucide-react";
import { useDeferredValue, useMemo } from "react";
import { Button } from "#/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "#/components/ui/select";
import { getAlbumGenreNames } from "#/lib/library-display";

const ALL_ARTISTS = "all";
const ALL_GENRES = "all";

export type AlbumFilterState = {
	albumQuery: string;
	artistId: string;
	genre: string;
};

export function collectAlbumGenres(albums: Album[]): string[] {
	const seen = new Set<string>();
	const out: string[] = [];
	for (const album of albums) {
		for (const genre of getAlbumGenreNames(album)) {
			const key = genre.toLowerCase();
			if (seen.has(key)) continue;
			seen.add(key);
			out.push(genre);
		}
	}
	return out.sort((a, b) =>
		a.localeCompare(b, undefined, { sensitivity: "base" }),
	);
}

export function filterAlbums(
	albums: Album[],
	filters: AlbumFilterState,
): Album[] {
	return albums.filter((album) => {
		if (filters.genre && filters.genre !== ALL_GENRES) {
			const genres = getAlbumGenreNames(album);
			if (
				!genres.some((g) => g.toLowerCase() === filters.genre.toLowerCase())
			) {
				return false;
			}
		}
		return true;
	});
}

export function AlbumFilters({
	artists,
	genreOptions,
	filters,
	onFiltersChange,
	resultCount,
	showSearch = true,
}: {
	artists: Artist[];
	genreOptions: string[];
	filters: AlbumFilterState;
	onFiltersChange: (next: AlbumFilterState) => void;
	resultCount?: number;
	showSearch?: boolean;
}) {
	const deferredQuery = useDeferredValue(filters.albumQuery);
	const hasActiveFilters =
		(showSearch && filters.albumQuery.trim() !== "") ||
		(filters.artistId !== "" && filters.artistId !== ALL_ARTISTS) ||
		(filters.genre !== "" && filters.genre !== ALL_GENRES);

	const artistValue =
		filters.artistId && filters.artistId !== ALL_ARTISTS
			? filters.artistId
			: ALL_ARTISTS;
	const genreValue =
		filters.genre && filters.genre !== ALL_GENRES ? filters.genre : ALL_GENRES;

	const sortedArtists = useMemo(
		() =>
			[...artists].sort((a, b) =>
				a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
			),
		[artists],
	);
	// Base UI's SelectValue shows an item's label only when it knows the items.
	const artistItems = useMemo(
		() => [
			{ value: ALL_ARTISTS, label: "All artists" },
			...sortedArtists.map((artist) => ({
				value: artist.id,
				label: artist.name,
			})),
		],
		[sortedArtists],
	);
	const genreItems = useMemo(
		() => [
			{ value: ALL_GENRES, label: "All genres" },
			...genreOptions.map((genre) => ({ value: genre, label: genre })),
		],
		[genreOptions],
	);

	const clearFilters = () => {
		onFiltersChange({
			albumQuery: showSearch ? "" : filters.albumQuery,
			artistId: ALL_ARTISTS,
			genre: ALL_GENRES,
		});
	};

	return (
		<div className="mb-6 space-y-3">
			<div className="flex flex-col gap-3 lg:flex-row lg:items-center">
				{showSearch ? (
					<SearchField
						className="min-w-0 flex-1"
						placeholder="Search albums…"
						value={filters.albumQuery}
						onChange={(e) =>
							onFiltersChange({ ...filters, albumQuery: e.target.value })
						}
					/>
				) : null}

				<div className="flex flex-wrap items-center gap-2">
					<div className="flex min-w-0 flex-col gap-1 text-caption text-xs">
						<span>Artist</span>
						<Select
							items={artistItems}
							value={artistValue}
							onValueChange={(artistId) =>
								onFiltersChange({
									...filters,
									artistId: artistId ?? ALL_ARTISTS,
								})
							}
						>
							<SelectTrigger className="w-45 max-w-full">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								{artistItems.map((item) => (
									<SelectItem key={item.value} value={item.value}>
										{item.label}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>

					<div className="flex min-w-0 flex-col gap-1 text-caption text-xs">
						<span>Genre</span>
						<Select
							items={genreItems}
							value={genreValue}
							onValueChange={(genre) =>
								onFiltersChange({ ...filters, genre: genre ?? ALL_GENRES })
							}
						>
							<SelectTrigger className="w-45 max-w-full">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								{genreItems.map((item) => (
									<SelectItem key={item.value} value={item.value}>
										{item.label}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>

					{hasActiveFilters ? (
						<Button
							type="button"
							variant="muted"
							size="sm"
							onClick={clearFilters}
						>
							<X className="size-4" />
							Clear
						</Button>
					) : null}
				</div>
			</div>

			{resultCount != null ? (
				<p className="text-caption text-xs">
					{resultCount} album{resultCount === 1 ? "" : "s"}
					{deferredQuery !== filters.albumQuery ? " · updating…" : ""}
				</p>
			) : null}
		</div>
	);
}
