import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
	within,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { stubIntersectionObserver } from "#/test-intersection-observer";
import { AlbumDetailContent } from "./-album-page";

const mocks = vi.hoisted(() => ({
	getAlbum: vi.fn(),
	playTrack: vi.fn(),
	queueTracks: vi.fn(),
	coverAccent: vi.fn(),
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
		listTrackPlayCounts: async () => new Map(),
		getAlbum: mocks.getAlbum,
		listAlbumOnlineGenres: async () => [],
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
	TopNavBackdrop: ({ children }: { children: React.ReactNode }) => (
		<div data-testid="top-nav-backdrop">{children}</div>
	),
	useCoverAccent: mocks.coverAccent,
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
		mocks.coverAccent.mockReturnValue({});
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

	it("runs the cover colour behind the Top Nav and scrolls it with the page", async () => {
		mocks.coverAccent.mockReturnValue({ "--cover-tint": "hsl(10 50% 55%)" });
		renderWithQuery(<AlbumDetailContent albumId="album-1" />);
		await screen.findByRole("heading", { name: "1989" });

		const wash = within(screen.getByTestId("top-nav-backdrop")).getByTestId(
			"top-nav-wash",
		);
		expect(wash.style.getPropertyValue("--cover-tint")).toBe("hsl(10 50% 55%)");

		const scroller = screen.getByTestId("album-detail-content")
			.parentElement as HTMLElement;
		scroller.scrollTop = 120;
		fireEvent.scroll(scroller);
		expect(wash.style.getPropertyValue("--cover-wash-scroll")).toBe("120px");
		// A child would inherit the Desktop Client's drag region past the bar.
		expect(wash.childElementCount).toBe(0);
	});

	it("leaves the Top Nav flat while the cover has no colour", async () => {
		renderWithQuery(<AlbumDetailContent albumId="album-1" />);
		await screen.findByRole("heading", { name: "1989" });
		expect(screen.queryByTestId("top-nav-wash")).toBeNull();
	});

	describe("sticky bar", () => {
		afterEach(() => {
			vi.unstubAllGlobals();
		});

		it("shows while the hero's actions are scrolled away, with the same actions", async () => {
			const observer = stubIntersectionObserver();
			renderWithQuery(<AlbumDetailContent albumId="album-1" />);
			await screen.findByRole("heading", { name: "1989" });
			expect(screen.queryByTestId("detail-sticky-bar")).toBeNull();
			expect(observer.options[0]?.root).toBe(
				screen.getByTestId("album-detail-content").parentElement,
			);

			act(() => observer.scroll("scrolled past"));
			const bar = within(screen.getByTestId("detail-sticky-bar"));
			expect(bar.getByText("1989")).toBeTruthy();
			expect(bar.getByText("Taylor Swift, Guest Artist")).toBeTruthy();
			// The artist alone: external links stay in the hero.
			expect(bar.getAllByRole("link")).toHaveLength(1);

			const source = {
				kind: "album",
				albumId: "album-1",
				albumTitle: "1989",
				artistName: "Taylor Swift, Guest Artist",
			};
			fireEvent.click(bar.getByRole("button", { name: "Play" }));
			expect(mocks.playTrack).toHaveBeenLastCalledWith(
				"track-1",
				["track-1"],
				source,
			);
			fireEvent.click(bar.getByRole("button", { name: "Queue album" }));
			expect(mocks.queueTracks).toHaveBeenLastCalledWith(["track-1"], source);

			act(() => observer.scroll("in view"));
			expect(screen.queryByTestId("detail-sticky-bar")).toBeNull();
		});

		it("stays away while the actions are still below the fold", async () => {
			const observer = stubIntersectionObserver();
			renderWithQuery(<AlbumDetailContent albumId="album-1" />);
			await screen.findByRole("heading", { name: "1989" });

			act(() => observer.scroll("below the fold"));
			expect(screen.queryByTestId("detail-sticky-bar")).toBeNull();
		});
	});
});
