import type { AlbumDetail } from "@repo/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";
import { AlbumDetailHeader } from "./album-detail-header";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
	...(await importOriginal<typeof import("@tanstack/react-router")>()),
	Link: ({
		children,
		to,
		params,
		...props
	}: {
		children: React.ReactNode;
		to: string;
		params?: Record<string, string>;
	}) => (
		<a
			href={Object.entries(params ?? {}).reduce(
				(href, [key, value]) => href.replace(`$${key}`, value),
				to,
			)}
			{...props}
		>
			{children}
		</a>
	),
}));

vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: () => true,
}));

vi.mock("#/hooks/use-delete-library", () => ({
	useDeleteAlbum: () => ({ mutate: vi.fn(), isPending: false }),
	usePreviewAlbumDeletion: () => ({ mutate: vi.fn(), isPending: false }),
}));

vi.mock("@repo/ui", async (importOriginal) => ({
	...(await importOriginal<typeof import("@repo/ui")>()),
	useLayout: () => ({ preferences: { theme: { mode: "dark" } } }),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	},
}));

const album = {
	id: "album-1",
	title: "Album 1",
	artistId: "artist-1",
	artistName: "Artist",
	albumArtists: [],
	releaseIdentifiers: [],
	genreItems: [],
	year: 2024,
	trackCount: 2,
	tracks: [
		{
			id: "track-1",
			title: "Track 1",
			artistName: "Artist",
			artists: [],
			albumId: "album-1",
			discNo: 1,
			durationMs: 120000,
			format: "flac",
			genres: [],
		},
		{
			id: "track-2",
			title: "Track 2",
			artistName: "Artist",
			artists: [],
			albumId: "album-1",
			discNo: 1,
			durationMs: 180000,
			format: "flac",
			genres: [],
		},
	],
} satisfies AlbumDetail;

describe("AlbumDetailHeader", () => {
	it("queues the album without starting playback", () => {
		const onPlayAlbum = vi.fn();
		const onQueueAlbum = vi.fn();

		render(
			<AlbumDetailHeader
				album={album}
				onPlayAlbum={onPlayAlbum}
				onQueueAlbum={onQueueAlbum}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Queue album" }));

		expect(onQueueAlbum).toHaveBeenCalledOnce();
		expect(onPlayAlbum).not.toHaveBeenCalled();
	});
});
