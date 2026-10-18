import type { AlbumDetail } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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

const listAlbumOnlineGenres = vi.hoisted(() => vi.fn());

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		listAlbumOnlineGenres,
	},
}));

function renderHeader(
	album: AlbumDetail,
	onPlayAlbum = vi.fn(),
	onQueueAlbum = vi.fn(),
) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<AlbumDetailHeader
				album={album}
				onPlayAlbum={onPlayAlbum}
				onQueueAlbum={onQueueAlbum}
			/>
		</QueryClientProvider>,
	);
}

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

		listAlbumOnlineGenres.mockResolvedValue([]);
		renderHeader(album, onPlayAlbum, onQueueAlbum);

		fireEvent.click(screen.getByRole("button", { name: "Queue album" }));

		expect(onQueueAlbum).toHaveBeenCalledOnce();
		expect(onPlayAlbum).not.toHaveBeenCalled();
	});

	it("shows Deezer's genres when the tags name none", async () => {
		listAlbumOnlineGenres.mockResolvedValue(["Rock", "Alternative"]);
		renderHeader(album);
		expect(await screen.findByText("Rock, Alternative")).toBeTruthy();
		expect(listAlbumOnlineGenres).toHaveBeenCalledWith("album-1");
	});

	it("shows the tags' genres without asking Deezer", () => {
		listAlbumOnlineGenres.mockClear();
		renderHeader({
			...album,
			genreItems: ["Metal", "Rock", "Pop", "Jazz"].map((name) => ({
				id: name,
				name,
			})),
		});
		expect(screen.getByText("Metal, Rock, Pop").title).toBe(
			"Metal, Rock, Pop, Jazz",
		);
		expect(listAlbumOnlineGenres).not.toHaveBeenCalled();
	});
});
