import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";
import {
	CollectionCoverCardStack,
	CollectionCoverStack,
	CollectionCoverStrip,
	pickPreviewTracks,
	pickRandomUniqueAlbumTracks,
	pickStableUniqueAlbumTracks,
} from "./collection-cover-strip";

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
	},
}));

const tracks = [
	{
		id: "t1",
		title: "A",
		albumId: "a1",
		artistName: "Artist",
		durationMs: 1,
		format: "flac",
	},
	{
		id: "t2",
		title: "B",
		albumId: "a2",
		artistName: "Artist",
		durationMs: 1,
		format: "flac",
	},
	{
		id: "t3",
		title: "C",
		albumId: "a3",
		artistName: "Artist",
		durationMs: 1,
		format: "flac",
	},
	{
		id: "t4",
		title: "D",
		albumId: "a4",
		artistName: "Artist",
		durationMs: 1,
		format: "flac",
	},
	{
		id: "t5",
		title: "E",
		albumId: "a5",
		artistName: "Artist",
		durationMs: 1,
		format: "flac",
	},
].map((track) => ({ ...track, artists: [], discNo: 1, genres: [] }));

describe("CollectionCoverStrip", () => {
	it("selects a stable four-track preview from a seed", () => {
		const first = pickPreviewTracks(tracks, "playlist-1").map(
			(track) => track.id,
		);
		const second = pickPreviewTracks(tracks, "playlist-1").map(
			(track) => track.id,
		);

		expect(first).toEqual(second);
		expect(first).toHaveLength(4);
		expect(first).not.toEqual(["t1", "t2", "t3", "t4"]);
	});

	it("renders a fit-content row of four album covers", () => {
		const { container } = render(
			<CollectionCoverStrip tracks={tracks} seed="genre-pop" layout="row" />,
		);

		const strip = container.firstElementChild;
		expect(strip?.className).toContain("w-fit");
		expect(strip?.className).toContain("flex");
		expect(container.querySelectorAll("img")).toHaveLength(4);
	});

	it("renders a two-by-two detail cover grid", () => {
		const { container } = render(
			<CollectionCoverStrip tracks={tracks} seed="genre-pop" layout="grid" />,
		);

		const strip = container.firstElementChild;
		expect(strip?.className).toContain("grid");
		expect(strip?.className).toContain("grid-cols-2");
		expect(container.querySelectorAll("img")).toHaveLength(4);
	});

	it("selects at most four random unique album covers for playlist stacks", () => {
		const selected = pickRandomUniqueAlbumTracks(
			[...tracks, { ...tracks[0], id: "t6" }, { ...tracks[1], id: "t7" }],
			4,
			() => 0.9,
		);

		expect(selected).toHaveLength(4);
		expect(new Set(selected.map((track) => track.albumId)).size).toBe(4);
	});

	it("keeps a seeded unique-album preview stable across track ordering", () => {
		const duplicateTrack = { ...tracks[0], id: "duplicate-album-track" };
		const first = pickStableUniqueAlbumTracks(
			[...tracks, duplicateTrack],
			"genre:Synthpop",
		);
		const reordered = pickStableUniqueAlbumTracks(
			[tracks[4], tracks[2], duplicateTrack, tracks[1], tracks[3], tracks[0]],
			"genre:Synthpop",
		);

		expect(first.map((track) => track.albumId)).toEqual(
			reordered.map((track) => track.albumId),
		);
		expect(first).toHaveLength(4);
		expect(new Set(first.map((track) => track.albumId)).size).toBe(4);
	});

	it("renders the same card cover identity for the same seed", () => {
		const { container: firstContainer } = render(
			<CollectionCoverCardStack tracks={tracks} seed="genre:Synthpop" />,
		);
		const { container: reorderedContainer } = render(
			<CollectionCoverCardStack
				tracks={[...tracks].reverse()}
				seed="genre:Synthpop"
			/>,
		);

		expect(
			Array.from(firstContainer.querySelectorAll("img"), (image) => image.src),
		).toEqual(
			Array.from(
				reorderedContainer.querySelectorAll("img"),
				(image) => image.src,
			),
		);
	});

	it("raises detail stack covers on hover", () => {
		const { container: detailContainer } = render(
			<CollectionCoverStack tracks={tracks} />,
		);

		for (const image of Array.from(detailContainer.querySelectorAll("img"))) {
			expect(image.className).toContain("transition");
			expect(image.className).toContain("hover:z-50");
			expect(image.className).toContain("hover:scale-105");
			expect(image.className).toContain("hover:rotate-0");
		}
	});

	it.each([
		[1, [["left-0", "z-40", "w-full"]]],
		[
			2,
			[
				["left-0", "z-40", "w-[70%]"],
				["left-[30%]", "z-30", "w-[70%]"],
			],
		],
		[
			3,
			[
				["left-0", "z-40", "w-[60%]"],
				["left-[20%]", "z-30", "w-[60%]"],
				["left-[40%]", "z-20", "w-[60%]"],
			],
		],
		[
			4,
			[
				["left-0", "z-40", "w-[50%]"],
				["left-[20%]", "z-30", "w-[50%]"],
				["left-[35%]", "z-20", "w-[50%]"],
				["left-[50%]", "z-10", "w-[50%]"],
			],
		],
	])("fits %i unique album covers across the card", (count, layout) => {
		const { container } = render(
			<CollectionCoverCardStack
				tracks={tracks.slice(0, count)}
				seed={`layout:${count}`}
			/>,
		);

		const covers = Array.from(
			container.querySelectorAll('[data-testid="collection-card-cover"]'),
		);
		expect(covers).toHaveLength(count);
		for (const [index, cover] of covers.entries()) {
			for (const className of layout[index]) {
				expect(cover.className).toContain(className);
			}
			expect(cover.className).toContain("top-0");
			expect(cover.className).toContain("h-[88%]");
		}
	});

	it("uses the unique album count for the card layout", () => {
		const duplicateAlbumTracks = [
			tracks[0],
			{ ...tracks[0], id: "duplicate-album-track" },
		];
		const { container } = render(
			<CollectionCoverCardStack
				tracks={duplicateAlbumTracks}
				seed="duplicate-albums"
			/>,
		);

		const covers = Array.from(
			container.querySelectorAll('[data-testid="collection-card-cover"]'),
		);
		expect(covers).toHaveLength(1);
		expect(covers[0].className).toContain("left-0");
		expect(covers[0].className).toContain("w-full");
	});

	it("expands only the hovered card cover across the artwork area", () => {
		const { container } = render(
			<CollectionCoverCardStack tracks={tracks} seed="hover-layout" />,
		);

		const covers = Array.from(
			container.querySelectorAll('[data-testid="collection-card-cover"]'),
		);
		expect(covers).toHaveLength(4);
		for (const cover of covers) {
			expect(cover.className).toContain(
				"transition-[left,width,box-shadow,filter]",
			);
			expect(cover.className).toContain("hover:left-0");
			expect(cover.className).toContain("hover:z-50");
			expect(cover.className).toContain("hover:w-full");
			expect(cover.className).not.toContain("hover:scale-105");
		}
	});

	it("renders card covers as restrained motion-aware layers", () => {
		const { container } = render(
			<CollectionCoverCardStack tracks={tracks} seed="polished-layers" />,
		);

		const covers = Array.from(
			container.querySelectorAll('[data-testid="collection-card-cover"]'),
		);
		expect(covers).toHaveLength(4);
		for (const cover of covers) {
			expect(cover.className).toContain("border-r");
			expect(cover.className).toContain("border-background/30");
			expect(cover.className).toContain(
				"shadow-[10px_0_20px_rgb(0_0_0_/_35%)]",
			);
			expect(cover.className).toContain("duration-[260ms]");
			expect(cover.className).toContain("hover:delay-[60ms]");
			expect(cover.className).toContain("motion-reduce:transition-none");
		}
	});

	it("reveals loaded card artwork over a muted placeholder", () => {
		const { container } = render(
			<CollectionCoverCardStack tracks={[tracks[0]]} seed="loading-art" />,
		);

		const image = container.querySelector("img");
		const placeholder = container.querySelector(
			'[data-testid="collection-card-cover-placeholder"]',
		);
		expect(image?.className).toContain("opacity-0");
		expect(placeholder?.className).toContain("animate-pulse");

		fireEvent.load(image as HTMLImageElement);

		expect(image?.className).toContain("opacity-100");
		expect(placeholder?.className).toContain("opacity-0");
	});

	it("reveals fallback artwork when a card cover fails", () => {
		const { container } = render(
			<CollectionCoverCardStack tracks={[tracks[0]]} seed="failed-art" />,
		);

		fireEvent.error(container.querySelector("img") as HTMLImageElement);

		const fallback = container.querySelector(
			'[data-testid="collection-card-cover"] > [aria-hidden="true"]:last-child',
		);
		const placeholder = container.querySelector(
			'[data-testid="collection-card-cover-placeholder"]',
		);
		expect(fallback?.textContent).toBe("A");
		expect(fallback?.className).toContain("opacity-100");
		expect(placeholder?.className).toContain("opacity-0");
	});
});
