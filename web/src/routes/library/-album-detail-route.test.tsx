import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { AlbumDetailContent } from "./-album-page";

const mocks = vi.hoisted(() => ({
	getAlbum: vi.fn(),
	playTrack: vi.fn(),
	queueTracks: vi.fn(),
}));

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

vi.mock("#/hooks/use-delete-library", () => {
	const idle = () => ({ mutate: vi.fn(), isPending: false });
	return {
		useDeleteTrack: idle,
		usePreviewTrackDeletion: idle,
		useDeleteAlbum: idle,
		usePreviewAlbumDeletion: idle,
	};
});

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbum: mocks.getAlbum,
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		listAlbums: vi.fn().mockResolvedValue({ items: [] }),
	},
}));

vi.mock("@repo/ui", () => ({
	AlbumArt: ({
		coverUrl,
		className,
	}: {
		coverUrl?: string | null;
		className?: string;
	}) => (coverUrl ? <img alt="" src={coverUrl} className={className} /> : null),
	useCoverAccent: () => ({}),
	useLayout: () => ({ preferences: { theme: { mode: "dark" } } }),
	useResolvedThemeMode: () => "dark",
	usePlayback: () => ({
		playTrack: mocks.playTrack,
		queueTracks: mocks.queueTracks,
	}),
}));

function renderWithQuery(ui: React.ReactElement) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
	);
}

describe("album detail route", () => {
	beforeEach(() => {
		mocks.getAlbum.mockResolvedValue({
			id: "album-1",
			title: "1989",
			artistId: "artist-1",
			artistName: "Legacy Album Artist",
			albumArtists: [
				{ id: "artist-1", name: "Taylor Swift" },
				{ id: "artist-2", name: "Guest Artist" },
			],
			year: 2014,
			releaseDate: "2014-10-27",
			trackCount: 1,
			durationMs: 180_000,
			genres: ["Pop"],
			genreItems: [{ id: "genre-1", name: "Pop" }],
			tracks: [
				{
					id: "track-1",
					title: "Style",
					artistName: "Taylor Swift",
					albumId: "album-1",
					durationMs: 180_000,
					format: "flac",
				},
			],
		});
	});

	afterEach(() => {
		cleanup();
	});

	it("renders album detail without the back to library link", async () => {
		renderWithQuery(<AlbumDetailContent albumId="album-1" />);

		await screen.findByRole("heading", { name: "1989" });
		expect(screen.getByText("Taylor Swift, Guest Artist")).toBeTruthy();
		expect(screen.getByText("2014-10-27")).toBeTruthy();
		expect(screen.queryByText("Legacy Album Artist")).toBeNull();
		expect(screen.queryByRole("link", { name: /Back to library/ })).toBeNull();
	});

	it("passes album provenance to album actions and track playback", async () => {
		renderWithQuery(<AlbumDetailContent albumId="album-1" />);
		await screen.findByRole("heading", { name: "1989" });
		const source = {
			kind: "album",
			albumId: "album-1",
			albumTitle: "1989",
			artistName: "Taylor Swift, Guest Artist",
		};
		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		expect(mocks.playTrack).toHaveBeenLastCalledWith(
			"track-1",
			["track-1"],
			source,
		);
		fireEvent.click(screen.getByRole("button", { name: "Queue album" }));
		expect(mocks.queueTracks).toHaveBeenLastCalledWith(["track-1"], source);
		fireEvent.click(screen.getByText("Style"));
		expect(mocks.playTrack).toHaveBeenLastCalledWith(
			"track-1",
			["track-1"],
			source,
		);
	});

	it("aligns its content with the Albums list page width", async () => {
		renderWithQuery(<AlbumDetailContent albumId="album-1" />);

		await screen.findByRole("heading", { name: "1989" });
		const content = screen.getByTestId("album-detail-content");
		expect(content.className).toContain("page-content-column");
		expect(content.parentElement?.className).toContain("pl-(--shell-inset)");
		expect(content.parentElement?.className).toContain("page-edge-right");
	});
});
