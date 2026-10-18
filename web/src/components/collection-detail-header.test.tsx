import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { CollectionDetailHeader } from "./collection-detail-header";

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	},
}));

const normalizedTrackFields = { artists: [], discNo: 1, genres: [] };

describe("CollectionDetailHeader", () => {
	afterEach(() => {
		cleanup();
	});

	it("renders collection actions and calls handlers when tracks exist", () => {
		const onPlay = vi.fn();
		const onShuffle = vi.fn();
		const onQueue = vi.fn();

		render(
			<CollectionDetailHeader
				kind="Playlist"
				title="Favorites"
				subtitle="Default playlist"
				metaTags={["2 tracks", "7m"]}
				trackCount={2}
				onPlay={onPlay}
				onShuffle={onShuffle}
				onQueue={onQueue}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Play" }));
		fireEvent.click(screen.getByRole("button", { name: "Shuffle" }));
		fireEvent.click(screen.getByRole("button", { name: "Queue" }));

		expect(onPlay).toHaveBeenCalledOnce();
		expect(onShuffle).toHaveBeenCalledOnce();
		expect(onQueue).toHaveBeenCalledOnce();
	});

	it("disables playback actions when collection has no tracks", () => {
		render(
			<CollectionDetailHeader
				kind="Genre"
				title="Synthpop"
				subtitle="No tracks"
				metaTags={["0 tracks"]}
				trackCount={0}
				onPlay={vi.fn()}
				onShuffle={vi.fn()}
				onQueue={vi.fn()}
			/>,
		);

		expect(
			screen.getByRole("button", { name: "Play" }).hasAttribute("disabled"),
		).toBe(true);
		expect(
			screen.getByRole("button", { name: "Shuffle" }).hasAttribute("disabled"),
		).toBe(true);
		expect(
			screen.getByRole("button", { name: "Queue" }).hasAttribute("disabled"),
		).toBe(true);
	});

	it("renders a stacked unique album cover for playlists", () => {
		const { container } = render(
			<CollectionDetailHeader
				kind="Playlist"
				title="Favorites"
				subtitle="Default playlist"
				metaTags={["4 tracks"]}
				trackCount={4}
				coverTracks={[
					{
						...normalizedTrackFields,
						id: "t1",
						title: "A",
						albumId: "a1",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
					{
						...normalizedTrackFields,
						id: "t2",
						title: "B",
						albumId: "a2",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
					{
						...normalizedTrackFields,
						id: "t3",
						title: "C",
						albumId: "a3",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
					{
						...normalizedTrackFields,
						id: "t4",
						title: "D",
						albumId: "a4",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
				]}
				onPlay={vi.fn()}
				onShuffle={vi.fn()}
				onQueue={vi.fn()}
			/>,
		);

		expect(container.querySelector(".grid-cols-2")).toBeNull();
		expect(
			container.querySelector('[data-testid="collection-cover-stack"]'),
		).toBeTruthy();
		expect(container.querySelectorAll("img")).toHaveLength(4);
		expect(
			new Set(
				Array.from(container.querySelectorAll("img")).map((img) =>
					img.getAttribute("src"),
				),
			),
		).toEqual(new Set(["/cover/a1", "/cover/a2", "/cover/a3", "/cover/a4"]));
		expect(
			container.querySelector('[data-testid="collection-cover-stack"]')
				?.className,
		).toContain("h-56");
		expect(
			container.querySelector('[data-testid="collection-cover-stack"]')
				?.className,
		).not.toContain("bg-muted");
	});

	it("deduplicates playlist stack covers by album", () => {
		const { container } = render(
			<CollectionDetailHeader
				kind="Playlist"
				title="Favorites"
				subtitle="Default playlist"
				metaTags={["3 tracks"]}
				trackCount={3}
				coverTracks={[
					{
						...normalizedTrackFields,
						id: "t1",
						title: "A",
						albumId: "a1",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
					{
						...normalizedTrackFields,
						id: "t2",
						title: "B",
						albumId: "a1",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
					{
						...normalizedTrackFields,
						id: "t3",
						title: "C",
						albumId: "a2",
						artistName: "Artist",
						durationMs: 1,
						format: "flac",
					},
				]}
				onPlay={vi.fn()}
				onShuffle={vi.fn()}
				onQueue={vi.fn()}
			/>,
		);

		expect(container.querySelectorAll("img")).toHaveLength(2);
	});

	it("renders right-aligned collection search when controlled search props are provided", () => {
		const onSearchChange = vi.fn();
		render(
			<CollectionDetailHeader
				kind="Genre"
				title="Synthpop"
				subtitle="Library genre"
				metaTags={["2 tracks"]}
				trackCount={2}
				searchValue=""
				searchPlaceholder="Search Synthpop…"
				onSearchChange={onSearchChange}
				onPlay={vi.fn()}
				onShuffle={vi.fn()}
				onQueue={vi.fn()}
			/>,
		);

		fireEvent.change(screen.getByPlaceholderText("Search Synthpop…"), {
			target: { value: "blue" },
		});

		expect(onSearchChange).toHaveBeenCalledWith("blue");
		expect(
			screen.getByPlaceholderText("Search Synthpop…").parentElement?.className,
		).toContain("ml-auto");
	});
});
