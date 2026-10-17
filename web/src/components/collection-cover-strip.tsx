import type { Track } from "@repo/api-client";
import { AlbumArt } from "@repo/ui";
import { useMemo, useState } from "react";
import { apiClient } from "#/lib/api";
import { cn } from "#/lib/utils";

const STACK_COVER_LIMIT = 4;
const CARD_COVER_LAYOUTS: Record<number, string[]> = {
	1: ["left-0 z-40 w-full"],
	2: ["left-0 z-40 w-[70%]", "left-[30%] z-30 w-[70%]"],
	3: [
		"left-0 z-40 w-[60%]",
		"left-[20%] z-30 w-[60%]",
		"left-[40%] z-20 w-[60%]",
	],
	4: [
		"left-0 z-40 w-[50%]",
		"left-[20%] z-30 w-[50%]",
		"left-[35%] z-20 w-[50%]",
		"left-[50%] z-10 w-[50%]",
	],
};
const CARD_COVER_LAYER_CLASS =
	"absolute top-0 h-11/12 overflow-hidden rounded-sm border-0 border-background/30 border-r bg-muted shadow-[10px_0_20px_rgb(0_0_0_/_35%)] transition-all delay-0 duration-[260ms] ease-[cubic-bezier(.22,.8,.24,1)] hover:left-0 hover:z-50 hover:w-full hover:rounded-sm hover:border-r-transparent hover:brightness-[1.025] hover:saturate-[1.04] hover:shadow-none hover:delay-[60ms] motion-reduce:delay-0 motion-reduce:transition-none";
const CARD_COVER_PLACEHOLDER_CLASS =
	"pointer-events-none absolute inset-0 bg-muted transition-opacity duration-150 ease-out motion-reduce:animate-none motion-reduce:transition-none";
const CARD_COVER_ART_CLASS =
	"relative size-full rounded-none object-cover text-sm transition-opacity duration-150 ease-out motion-reduce:transition-none";

function hash(value: string): number {
	let out = 0;
	for (let i = 0; i < value.length; i += 1) {
		out = (out * 31 + value.charCodeAt(i)) >>> 0;
	}
	return out;
}

export function pickPreviewTracks<T extends { id: string }>(
	tracks: T[],
	seed: string,
	limit = 4,
): T[] {
	const next = [...tracks];
	let state = hash(seed) || 1;
	for (let i = next.length - 1; i > 0; i -= 1) {
		state = (state * 1664525 + 1013904223) >>> 0;
		const j = state % (i + 1);
		[next[i], next[j]] = [next[j], next[i]];
	}
	return next.slice(0, limit);
}

export function CollectionCoverStrip({
	tracks,
	seed,
	layout = "row",
	className,
}: {
	tracks: Track[];
	seed: string;
	layout?: "row" | "grid";
	className?: string;
}) {
	const preview = pickPreviewTracks(tracks, seed);
	const isGrid = layout === "grid";

	return (
		<div
			className={cn(
				isGrid
					? "grid size-32 shrink-0 grid-cols-2 gap-1 overflow-hidden rounded-md bg-muted p-1 shadow-lg"
					: "flex w-fit shrink-0 items-center gap-1",
				className,
			)}
		>
			{preview.map((track) => (
				<AlbumArt
					key={track.id}
					coverUrl={apiClient.getAlbumCoverUrl(track.albumId, 128)}
					title={track.title}
					className={cn(
						isGrid
							? "size-full rounded-sm text-sm"
							: "size-10 rounded-sm text-xs",
					)}
				/>
			))}
		</div>
	);
}

export function CollectionCoverStack({
	tracks,
	className,
}: {
	tracks: Track[];
	className?: string;
}) {
	const preview = useMemo(() => pickRandomUniqueAlbumTracks(tracks), [tracks]);
	const rotations = ["-rotate-10", "-rotate-3", "rotate-4", "rotate-10"];
	const offsets = [
		"left-[2%] top-[16%] z-10 h-[72%] w-[72%]",
		"left-[15%] top-[6%] z-20 h-[74%] w-[74%]",
		"left-[28%] top-[15%] z-30 h-[72%] w-[72%]",
		"left-[14%] top-[24%] z-40 h-[76%] w-[76%]",
	];

	return (
		<div
			data-testid="collection-cover-stack"
			className={cn(
				"relative h-56 w-56 shrink-0 sm:h-full sm:min-h-56",
				className,
			)}
		>
			{preview.map((track, index) => (
				<AlbumArt
					key={`${track.albumId}-${track.id}`}
					coverUrl={apiClient.getAlbumCoverUrl(track.albumId, 512)}
					title={track.title}
					className={cn(
						"absolute rounded-sm object-cover text-sm shadow-xl transition duration-200 ease-out hover:z-50 hover:scale-105 hover:rotate-0 hover:shadow-xl",
						offsets[index] ?? offsets.at(-1),
						rotations[index] ?? "rotate-0",
					)}
				/>
			))}
		</div>
	);
}

function CollectionCoverCardArt({
	track,
	className,
}: {
	track: Track;
	className?: string;
}) {
	const [hasSettled, setHasSettled] = useState(false);

	return (
		<div
			data-testid="collection-card-cover"
			onLoadCapture={() => setHasSettled(true)}
			onErrorCapture={() => setHasSettled(true)}
			className={cn(CARD_COVER_LAYER_CLASS, className)}
		>
			<div
				data-testid="collection-card-cover-placeholder"
				className={cn(
					CARD_COVER_PLACEHOLDER_CLASS,
					hasSettled ? "opacity-0" : "animate-pulse opacity-100",
				)}
				aria-hidden
			/>
			<AlbumArt
				coverUrl={apiClient.getAlbumCoverUrl(track.albumId, 512)}
				title={track.title}
				className={cn(
					CARD_COVER_ART_CLASS,
					hasSettled ? "opacity-100" : "opacity-0",
				)}
			/>
		</div>
	);
}

export function CollectionCoverCardStack({
	tracks,
	seed,
	className,
}: {
	tracks: Track[];
	seed: string;
	className?: string;
}) {
	const preview = useMemo(
		() => pickStableUniqueAlbumTracks(tracks, seed),
		[tracks, seed],
	);
	const layout = CARD_COVER_LAYOUTS[preview.length] ?? [];

	return (
		<div
			data-testid="playlist-card-cover-stack"
			className={cn("absolute inset-0 overflow-hidden bg-muted", className)}
		>
			{preview.map((track, index) => (
				<CollectionCoverCardArt
					key={`${track.albumId}-${track.id}`}
					track={track}
					className={layout[index]}
				/>
			))}
		</div>
	);
}

export function pickRandomUniqueAlbumTracks(
	tracks: Track[],
	limit = STACK_COVER_LIMIT,
	random: () => number = Math.random,
): Track[] {
	const seenAlbumIds = new Set<string>();
	const uniqueTracks: Track[] = [];
	for (const track of tracks) {
		if (seenAlbumIds.has(track.albumId)) continue;
		seenAlbumIds.add(track.albumId);
		uniqueTracks.push(track);
	}

	for (let i = uniqueTracks.length - 1; i > 0; i -= 1) {
		const j = Math.floor(random() * (i + 1));
		[uniqueTracks[i], uniqueTracks[j]] = [uniqueTracks[j], uniqueTracks[i]];
	}

	return uniqueTracks.slice(0, limit);
}

export function pickStableUniqueAlbumTracks<
	T extends { id: string; albumId: string },
>(tracks: T[], seed: string, limit = STACK_COVER_LIMIT): T[] {
	const seenAlbumIds = new Set<string>();
	const uniqueTracks: T[] = [];
	for (const track of tracks) {
		if (seenAlbumIds.has(track.albumId)) continue;
		seenAlbumIds.add(track.albumId);
		uniqueTracks.push(track);
	}

	uniqueTracks.sort((first, second) => {
		if (first.albumId < second.albumId) return -1;
		if (first.albumId > second.albumId) return 1;
		return 0;
	});
	return pickPreviewTracks(uniqueTracks, seed, limit);
}
