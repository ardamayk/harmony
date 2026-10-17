import { AlbumArt } from "@repo/ui";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useRef } from "react";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";
import { getAlbumArtistName } from "#/lib/library-display";
import { sortAlbumsByYear } from "#/lib/sort-albums";

export function MoreFromArtist({
	artistId,
	artistName,
	excludeAlbumId,
}: {
	artistId: string;
	artistName: string;
	excludeAlbumId: string;
}) {
	const scrollRef = useRef<HTMLDivElement>(null);
	const albums = useQuery({
		queryKey: ["library", "albums", "artist", artistId],
		queryFn: () => apiClient.listAlbums({ artistId, limit: 100 }),
		staleTime: 60_000,
	});

	const items = sortAlbumsByYear(
		(albums.data?.items ?? []).filter((album) => album.id !== excludeAlbumId),
	);

	if (albums.isLoading) {
		return (
			<section className="mt-6">
				<h2 className="mb-4 font-semibold text-heading text-xl">
					More from {artistName}
				</h2>
				<p className="text-foreground text-sm">Loading albums…</p>
			</section>
		);
	}

	if (items.length === 0) {
		return null;
	}

	const scrollBy = (direction: -1 | 1) => {
		const node = scrollRef.current;
		if (!node) return;
		node.scrollBy({ left: direction * 320, behavior: "smooth" });
	};

	return (
		<section className="mt-6">
			<div className="mb-4 flex items-center justify-between gap-3">
				<h2 className="font-semibold text-heading text-xl">
					More from {artistName}
				</h2>
				<div className="flex items-center gap-1">
					<Button
						type="button"
						variant="outline"
						size="icon"
						className="size-8"
						aria-label="Scroll left"
						onClick={() => scrollBy(-1)}
					>
						<ChevronLeft className="size-4" />
					</Button>
					<Button
						type="button"
						variant="outline"
						size="icon"
						className="size-8"
						aria-label="Scroll right"
						onClick={() => scrollBy(1)}
					>
						<ChevronRight className="size-4" />
					</Button>
				</div>
			</div>

			<div
				ref={scrollRef}
				className="flex gap-4 overflow-x-auto pt-2 pb-6 scrollbar-hidden"
			>
				{items.map((album) => (
					<Link
						key={album.id}
						to="/library/$albumId"
						params={{ albumId: album.id }}
						className="group w-40 shrink-0 rounded-sm outline-none sm:w-44"
					>
						<div className="mb-3 aspect-square transform-gpu overflow-hidden rounded-md border border-border backface-hidden transition duration-300 ease-out group-hover:-translate-y-1 group-hover:border-shell-active-foreground/35 group-hover:shadow-[0_18px_40px_-18px_rgb(0_0_0/0.75)] group-focus-visible:-translate-y-1 group-focus-visible:border-shell-active-foreground/60 group-focus-visible:shadow-[0_18px_40px_-18px_rgb(0_0_0/0.75)]">
							<AlbumArt
								coverUrl={apiClient.getAlbumCoverUrl(album.id, 512)}
								title={album.title}
								className="size-full transform-gpu text-2xl transition duration-500 ease-out group-hover:scale-104 group-focus-visible:scale-104"
							/>
						</div>
						<p
							className="truncate font-medium text-heading text-sm"
							title={album.title}
						>
							{album.title}
						</p>
						<p className="truncate text-caption text-xs">
							{getAlbumArtistName(album)}
							{album.year != null ? ` · ${album.year}` : ""}
						</p>
					</Link>
				))}
			</div>
		</section>
	);
}
