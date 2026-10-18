import type { Album } from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { Link } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight } from "lucide-react";
import {
	type ReactNode,
	useCallback,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";
import { getAlbumArtistName } from "#/lib/library-display";

/**
 * Albums in one horizontal row, scrolled with arrows beside the heading:
 * the Album page's "More from" and the Artist page's Discography. An arrow
 * is disabled when the row cannot scroll that way, like the Player Bar's
 * previous and next.
 */
export function AlbumShelf({
	heading,
	filters,
	albums,
}: {
	/** The section's title (an h2), with anything that sits beside it. */
	heading: ReactNode;
	/** Shown between the heading and the row, such as filter buttons. */
	filters?: ReactNode;
	albums: Album[];
}) {
	const scrollRef = useRef<HTMLDivElement>(null);
	// Both ends until measured, so a row that fits never offers an arrow.
	const [edges, setEdges] = useState({ start: true, end: true });
	const measure = useCallback(() => {
		const node = scrollRef.current;
		if (!node) return;
		// One pixel of slack: scroll positions can be fractional.
		const start = node.scrollLeft <= 1;
		const end = node.scrollLeft + node.clientWidth >= node.scrollWidth - 1;
		setEdges((previous) =>
			previous.start === start && previous.end === end
				? previous
				: { start, end },
		);
	}, []);
	// Measured again when the row's width or its Albums change.
	useLayoutEffect(() => {
		measure();
		const node = scrollRef.current;
		if (!node || typeof ResizeObserver === "undefined") return undefined;
		const observer = new ResizeObserver(measure);
		observer.observe(node);
		return () => observer.disconnect();
	}, [measure, albums]);
	const scrollBy = (direction: -1 | 1) => {
		scrollRef.current?.scrollBy({ left: direction * 320, behavior: "smooth" });
	};

	return (
		<>
			<div className="mb-4 flex items-center justify-between gap-3">
				{heading}
				<div className="flex items-center gap-1">
					<Button
						type="button"
						variant="outline"
						size="icon"
						className="size-8"
						aria-label="Scroll left"
						disabled={edges.start}
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
						disabled={edges.end}
						onClick={() => scrollBy(1)}
					>
						<ChevronRight className="size-4" />
					</Button>
				</div>
			</div>
			{filters}
			<div
				ref={scrollRef}
				onScroll={measure}
				className="flex gap-4 overflow-x-auto pt-2 pb-6 scrollbar-hidden"
			>
				{albums.map((album) => (
					<Link
						key={album.id}
						to="/library/$albumId"
						params={{ albumId: album.id }}
						className="group w-40 shrink-0 rounded-sm outline-none sm:w-44"
					>
						<div className="mb-3 aspect-square transform-gpu overflow-hidden rounded-md border border-border backface-hidden transition duration-300 ease-out group-hover:-translate-y-1 group-hover:border-heading/35 group-hover:shadow-card-lift group-focus-visible:-translate-y-1 group-focus-visible:border-heading/60 group-focus-visible:shadow-card-lift">
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
		</>
	);
}
