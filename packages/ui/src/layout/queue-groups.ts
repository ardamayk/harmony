import type { QueueItem, QueueItemSource } from "@repo/api-client";

export type QueueGroup = {
	source: QueueItemSource;
	items: QueueItem[];
	startIndex: number;
};

export function findCurrentQueueIndex(
	queue: QueueItem[],
	queueItemId?: string,
	trackId?: string,
): number {
	if (queueItemId) {
		const index = queue.findIndex((item) => item.id === queueItemId);
		if (index !== -1) return index;
	}
	return trackId ? queue.findIndex((item) => item.track.id === trackId) : -1;
}

function getSourceKey(source: QueueItemSource): string {
	switch (source.kind) {
		case "album":
			return JSON.stringify([
				source.kind,
				source.albumId,
				source.albumTitle,
				source.artistName,
			]);
		case "playlist":
			return JSON.stringify([source.kind, source.playlistId, source.name]);
		case "suggestion":
			return JSON.stringify([source.kind, source.basedOn]);
		case "user":
			return source.kind;
	}
}

export function groupQueueItems(queue: QueueItem[]): QueueGroup[] {
	const groups: QueueGroup[] = [];
	let previousKey: string | undefined;
	queue.forEach((item, index) => {
		const source = item.source ?? { kind: "user" };
		const key = getSourceKey(source);
		const previous = groups[groups.length - 1];
		if (previous && key === previousKey) previous.items.push(item);
		else groups.push({ source, items: [item], startIndex: index });
		previousKey = key;
	});
	return groups;
}

export function formatQueueDuration(milliseconds: number): string {
	const seconds = Math.floor(Math.max(0, milliseconds || 0) / 1000);
	return `${Math.floor(seconds / 60)}:${(seconds % 60).toString().padStart(2, "0")}`;
}

export function getQueueMinutes(items: QueueItem[]): number {
	return Math.ceil(
		items.reduce(
			(total, item) => total + Math.max(0, item.track.durationMs || 0),
			0,
		) / 60000,
	);
}

export function getQueueGroupDetails(
	group: QueueGroup,
	queue: QueueItem[],
	sourceTitles: Record<string, string | null> = {},
) {
	const { source, items } = group;
	const count = `${items.length} ${items.length === 1 ? "track" : "tracks"}`;
	switch (source.kind) {
		case "album":
			return {
				title: source.albumTitle,
				meta: `From album · ${source.artistName} · ${count}`,
				// The ring's slate: a source marker, never the accent.
				color: "var(--ring)",
				href: `/library/${encodeURIComponent(source.albumId)}`,
			};
		case "playlist":
			return {
				title: source.name,
				meta: `From playlist · ${count}`,
				color: "var(--ring)",
				href: `/playlists/${encodeURIComponent(source.playlistId)}`,
			};
		case "suggestion": {
			const titles = source.basedOn.map(
				(trackId) =>
					queue.find((item) => item.track.id === trackId)?.track.title ??
					sourceTitles[trackId] ??
					(trackId in sourceTitles ? "Unavailable track" : "Loading track…"),
			);
			return {
				title: "For you",
				meta: titles.length
					? `Based on ${titles.join(", ")}`
					: "Based on your listening",
				color: "var(--player-control-primary)",
			};
		}
		case "user":
			return {
				title: "Added by you",
				meta: `${count} · ${getQueueMinutes(items)} min`,
				color: "var(--caption)",
			};
	}
}
