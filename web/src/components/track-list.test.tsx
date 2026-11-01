import type { QueueItemSource, Track } from "@repo/api-client";
import { ListenTogetherContext } from "@repo/ui";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
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
import { TrackList } from "./track-list";

const toggleFavorite = vi.fn();
const playTrack = vi.fn();
const addToQueue = vi.fn();
const playNext = vi.fn();
const toastError = vi.hoisted(() => vi.fn());
const deleteTrack = vi.fn();
const previewTrackDeletion = vi.fn();
let favorite = false;
let hasDeletionCapability = true;

vi.mock("@repo/ui", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@repo/ui")>();
	return {
		...actual,
		toast: { ...actual.toast, error: toastError },
		usePlayback: () => ({
			playTrack,
			addToQueue,
			playNext,
			currentTrack: null,
			getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		}),
	};
});

vi.mock("#/hooks/use-favorite-tracks", () => ({
	useFavoriteTracks: () => ({
		isFavorite: () => favorite,
		toggleFavorite,
	}),
}));

vi.mock("#/hooks/use-delete-library", () => ({
	useDeleteTrack: () => ({ mutate: deleteTrack, isPending: false }),
	usePreviewTrackDeletion: () => ({
		mutate: previewTrackDeletion,
		isPending: false,
	}),
}));

vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: () => hasDeletionCapability,
}));

const openReplacement = vi.fn();
const confirmReplacement = vi.fn();
const cancelReplacement = vi.fn();
let replacementFlow: Record<string, unknown> = {};

vi.mock("#/hooks/use-track-replacement-flow", () => ({
	useTrackReplacementFlow: () => ({
		track: null,
		step: "select",
		preview: null,
		progress: 0,
		error: null,
		isBusy: false,
		isDesktop: false,
		open: openReplacement,
		cancel: cancelReplacement,
		replaceWith: vi.fn(),
		selectDesktopFile: vi.fn(),
		confirm: confirmReplacement,
		close: vi.fn(),
		...replacementFlow,
	}),
}));

const sampleTrack: Track = {
	id: "t1",
	title: "Welcome to New York",
	artistName: "Taylor Swift",
	albumId: "a1",
	albumTitle: "1989",
	trackNo: 1,
	discNo: 1,
	durationMs: 212_000,
	format: "flac",
	genre: "Pop",
	artists: [],
	genres: [{ id: "genre-pop", name: "Pop" }],
	bitDepth: 24,
	sampleRateHz: 96_000,
	bitrateKbps: 1856,
	sizeBytes: 50_059_000,
	replayGain: {
		trackGainDb: -7.25,
		trackPeak: 0.98,
		albumGainDb: -6.5,
		albumPeak: 1.01,
	},
	titleSort: "welcome to new york",
	filePath: "/managed/t1.flac",
	fileMtime: 1_700_000_000,
	contentSha256: "ab".repeat(32),
	revision: 2,
	createdAt: "2026-09-05T15:02:17Z",
	updatedAt: "2026-09-05T15:02:17Z",
};

describe("TrackList", () => {
	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
	});

	beforeEach(() => {
		favorite = false;
		hasDeletionCapability = true;
		replacementFlow = {};
		openReplacement.mockReset();
		confirmReplacement.mockReset();
		cancelReplacement.mockReset();
		playTrack.mockClear();
		addToQueue.mockReset().mockResolvedValue(undefined);
		playNext.mockReset().mockResolvedValue(undefined);
		toastError.mockClear();
		toggleFavorite.mockClear();
		deleteTrack.mockClear();
		previewTrackDeletion.mockReset();
		previewTrackDeletion.mockImplementation(
			(
				_trackId: string,
				options: { onSuccess: (preview: unknown) => void },
			) => {
				options.onSuccess({
					trackId: "t1",
					trackTitle: "Welcome to New York",
					managedFile: {
						path: "library/taylor-swift/1989/01-welcome.flac",
						sizeBytes: 50_059_000,
					},
					playlistReferences: [{ id: "p1", name: "Road Trip" }],
					queueReferences: [{ userId: "user", itemCount: 2 }],
					confirmationToken: "confirm-token",
				});
			},
		);
	});

	it("hides what a Guest may not use and keeps the details", async () => {
		const guest = {
			role: "guest",
			isAvailable: true,
			room: null,
			ending: null,
			openRoom: vi.fn(),
			closeRoom: vi.fn(),
			leaveRoom: vi.fn(),
			joinRoom: vi.fn(),
		} as const;
		render(
			<ListenTogetherContext.Provider value={guest}>
				<TrackList tracks={[sampleTrack]} showFavorite />
			</ListenTogetherContext.Provider>,
		);
		const row = screen.getByRole("row", { name: /Welcome to New York/ });

		expect(screen.queryByRole("button", { name: /favorites/ })).toBeNull();
		fireEvent.contextMenu(row);

		expect(
			await screen.findByRole("menuitem", { name: "Details" }),
		).toBeTruthy();
		for (const name of [
			"Play next",
			"Add to queue",
			"Replace file",
			"Delete track",
		]) {
			expect(screen.queryByRole("menuitem", { name })).toBeNull();
		}
	});

	it("renders album tracks with only the title line", () => {
		const { container } = render(
			<TrackList tracks={[sampleTrack]} albumId="a1" showMeta compact />,
		);
		const row = screen.getByRole("row", { name: /Welcome to New York/ });
		const cover = container.querySelector('img[src="/cover/a1"]');

		expect(screen.getByText("Welcome to New York")).toBeTruthy();
		expect(cover).toBeNull();
		expect(row.textContent).not.toContain("Taylor Swift");
		expect(screen.queryByText("Pop · FLAC · 24-bit · 96 kHz")).toBeNull();
		expect(screen.queryByText(/FLAC/)).toBeNull();
	});

	it.each<QueueItemSource | undefined>([
		{
			kind: "album",
			albumId: "a1",
			albumTitle: "1989",
			artistName: "Taylor Swift",
		},
		{ kind: "playlist", playlistId: "p1", name: "Favorites" },
		undefined,
	])(
		"uses individual queue actions without propagating collection provenance %j",
		async (source) => {
			render(<TrackList tracks={[sampleTrack]} source={source} />);
			const row = screen.getByRole("row", { name: /Welcome to New York/ });
			fireEvent.contextMenu(row);
			fireEvent.click(
				await screen.findByRole("menuitem", { name: "Add to queue" }),
			);
			await waitFor(() =>
				expect(addToQueue).toHaveBeenCalledExactlyOnceWith(sampleTrack.id),
			);
			fireEvent.contextMenu(row);
			fireEvent.click(
				await screen.findByRole("menuitem", { name: "Play next" }),
			);
			await waitFor(() =>
				expect(playNext).toHaveBeenCalledExactlyOnceWith(sampleTrack.id),
			);
			expect(playTrack).not.toHaveBeenCalled();
		},
	);

	it.each([
		{
			label: "Add to queue",
			action: addToQueue,
			message: "Failed to add track to queue",
		},
		{
			label: "Play next",
			action: playNext,
			message: "Failed to play track next",
		},
	])(
		"reports a failed $label action with track context",
		async ({ label, action, message }) => {
			const error = new Error("Network unavailable");
			action.mockRejectedValueOnce(error);
			const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
			render(<TrackList tracks={[sampleTrack]} />);
			fireEvent.contextMenu(
				screen.getByRole("row", { name: /Welcome to New York/ }),
			);
			fireEvent.click(await screen.findByRole("menuitem", { name: label }));
			await waitFor(() => expect(toastError).toHaveBeenCalledWith(message));
			expect(warning).toHaveBeenCalledWith(message, {
				trackId: sampleTrack.id,
				error,
			});
		},
	);

	it("renders non-album tracks with the artist line", () => {
		const { container } = render(<TrackList tracks={[sampleTrack]} compact />);
		const cover = container.querySelector('img[src="/cover/a1"]');

		expect(screen.getByText("Welcome to New York")).toBeTruthy();
		expect(cover).toBeTruthy();
		expect(cover?.className).toContain("size-8");
		expect(screen.getByText("Taylor Swift")).toBeTruthy();
	});

	it("shows a Plays column only when there are play counts", () => {
		const secondTrack = { ...sampleTrack, id: "t2", title: "Style" };
		const { rerender } = render(
			<TrackList tracks={[sampleTrack, secondTrack]} />,
		);
		expect(screen.queryByText("Plays")).toBeNull();
		rerender(
			<TrackList
				tracks={[sampleTrack, secondTrack]}
				playCounts={new Map([[sampleTrack.id, 11940672]])}
			/>,
		);
		expect(screen.getByText("Plays")).toBeTruthy();
		expect(screen.getByText((11940672).toLocaleString())).toBeTruthy();
	});

	it("renders ordered Track Artist relationships", () => {
		render(
			<TrackList
				tracks={[
					{
						...sampleTrack,
						artistName: "Legacy / Guess",
						artists: [
							{ id: "artist-1", name: "Earth, Wind & Fire" },
							{ id: "artist-2", name: "Guest / Artist" },
						],
					},
				]}
				compact
			/>,
		);

		expect(screen.getByText("Earth, Wind & Fire, Guest / Artist")).toBeTruthy();
		expect(screen.queryByText("Legacy / Guess")).toBeNull();
	});

	it("uses visible list numbering when requested", () => {
		const secondTrack = {
			...sampleTrack,
			id: "t2",
			title: "Style",
			albumId: "a2",
			trackNo: 1,
		};

		render(<TrackList tracks={[sampleTrack, secondTrack]} numbering="list" />);

		expect(
			screen.getByRole("row", { name: /1 Welcome to New York/ }),
		).toBeTruthy();
		expect(screen.getByRole("row", { name: /2 Style/ })).toBeTruthy();
	});

	it("shows disc and track positions for multi-disc albums", () => {
		const secondDiscTrack = {
			...sampleTrack,
			id: "t2",
			title: "Second Disc Song",
			discNo: 2,
		};

		render(<TrackList tracks={[sampleTrack, secondDiscTrack]} albumId="a1" />);

		expect(
			screen.getByRole("row", { name: /1\.1 Welcome to New York/ }),
		).toBeTruthy();
		expect(
			screen.getByRole("row", { name: /2\.1 Second Disc Song/ }),
		).toBeTruthy();
	});

	it("toggles favorites through the server-backed favorites hook", () => {
		render(
			<TrackList tracks={[sampleTrack]} albumId="a1" showFavorite compact />,
		);

		fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));

		expect(toggleFavorite).toHaveBeenCalledWith(sampleTrack.id);
	});

	it("renders filled favorite state", () => {
		favorite = true;

		render(
			<TrackList tracks={[sampleTrack]} albumId="a1" showFavorite compact />,
		);

		expect(
			screen.getByRole("button", { name: "Remove from favorites" }),
		).toBeTruthy();
	});

	it("plays with context tracks on double-click when configured", () => {
		const secondTrack = { ...sampleTrack, id: "t2", title: "Style" };

		render(
			<TrackList
				tracks={[sampleTrack, secondTrack]}
				contextTracks={[secondTrack, sampleTrack]}
				playMode="double"
			/>,
		);

		fireEvent.click(screen.getByText("Style"));
		expect(playTrack).not.toHaveBeenCalled();

		fireEvent.doubleClick(screen.getByText("Style"));
		expect(playTrack).toHaveBeenCalledWith("t2", ["t2", "t1"]);
	});

	it("plays on a tap on the phone shell, where a touch has no double click", () => {
		vi.stubGlobal("matchMedia", () => ({
			matches: true,
			addEventListener: () => {},
			removeEventListener: () => {},
		}));
		try {
			render(<TrackList tracks={[sampleTrack]} playMode="double" />);
			fireEvent.click(screen.getByText(sampleTrack.title));
			expect(playTrack).toHaveBeenCalledWith("t1", ["t1"]);
		} finally {
			vi.unstubAllGlobals();
		}
	});

	it("starts the Queue at the selected Track so no earlier Track counts as played", () => {
		const second = { ...sampleTrack, id: "t2", title: "Style" };
		const third = { ...sampleTrack, id: "t3", title: "Blank Space" };

		render(<TrackList tracks={[sampleTrack, second, third]} />);

		fireEvent.click(screen.getByText("Style"));

		expect(playTrack).toHaveBeenCalledWith("t2", ["t2", "t3"]);
	});

	it("plays the focused row with Enter in double-click mode", () => {
		render(<TrackList tracks={[sampleTrack]} playMode="double" />);

		fireEvent.keyDown(
			screen.getByRole("row", { name: /Welcome to New York/ }),
			{
				key: "Enter",
			},
		);

		expect(playTrack).toHaveBeenCalledWith("t1", ["t1"]);
	});

	it("hides destructive delete when disabled", () => {
		render(<TrackList tracks={[sampleTrack]} showDelete={false} />);

		expect(screen.queryByText("Delete track")).toBeNull();
	});

	it("offers Track Replacement only for managed Tracks with the server capability", () => {
		render(<TrackList tracks={[sampleTrack]} />);
		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Replace file"));
		expect(openReplacement).toHaveBeenCalledWith(sampleTrack);

		cleanup();
		hasDeletionCapability = false;
		render(<TrackList tracks={[sampleTrack]} />);
		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		expect(screen.queryByText("Replace file")).toBeNull();
	});

	it("shows every Track Replacement consequence before confirming", () => {
		replacementFlow = {
			track: sampleTrack,
			step: "review",
			preview: {
				trackId: "t1",
				trackTitle: "Welcome to New York",
				sourceFormat: {
					field: "format",
					current: "mp3",
					replacement: "flac",
					isChanged: true,
				},
				technicalProperties: [
					{
						field: "bitDepth",
						current: "",
						replacement: "24",
						isChanged: true,
					},
				],
				metadata: [
					{
						field: "genres",
						current: "Pop",
						replacement: "Synthpop",
						isChanged: true,
					},
				],
				library: {
					currentAlbumId: "a1",
					movesAlbum: true,
					createsAlbum: true,
					removesEmptyAlbum: true,
					removesEmptyArtists: ["Old Artist"],
					createsArtists: ["New Artist"],
					createsGenres: [],
				},
				artwork: {
					currentMediaType: "image/png",
					currentSha256: "a",
					replacementMediaType: "image/jpeg",
					replacementSha256: "b",
					isChanged: true,
					replacesAlbumArtwork: false,
				},
				canonicalPath: {
					field: "canonicalPath",
					current: "library/taylor-swift/1989/01-welcome.mp3",
					replacement: "library/new-artist/album/01-welcome.flac",
					isChanged: true,
				},
				oldFile: {
					path: "library/taylor-swift/1989/01-welcome.mp3",
					sizeBytes: 50_059_000,
				},
				playlistReferences: [{ id: "p1", name: "Road Trip" }],
				queueReferences: [{ userId: "user-1", itemCount: 2 }],
				confirmationToken: "token-1",
			},
		};
		render(<TrackList tracks={[sampleTrack]} />);

		const dialog = screen.getByRole("dialog", {
			name: "Replace Welcome to New York",
		});
		expect(within(dialog).getByText("mp3")).toBeTruthy();
		expect(within(dialog).getByText("flac")).toBeTruthy();
		expect(within(dialog).getByText("Synthpop")).toBeTruthy();
		expect(
			within(dialog).getByText("Moves the Track into a new Album"),
		).toBeTruthy();
		expect(within(dialog).getByText("Removes the emptied Album")).toBeTruthy();
		expect(
			within(dialog).getByText("Removes unreferenced Artists: Old Artist"),
		).toBeTruthy();
		expect(
			within(dialog).getByText(/Embedded artwork changes from image\/png to/),
		).toBeTruthy();
		expect(
			within(dialog).getByText("library/new-artist/album/01-welcome.flac"),
		).toBeTruthy();
		expect(
			within(dialog).getByText(/will be deleted permanently/),
		).toBeTruthy();
		expect(within(dialog).getByText("Road Trip")).toBeTruthy();
		expect(within(dialog).getByText("2 Queue items")).toBeTruthy();

		fireEvent.click(
			within(dialog).getByRole("button", { name: "Replace track" }),
		);
		expect(confirmReplacement).toHaveBeenCalledTimes(1);
	});

	it("hides permanent deletion when the server capability is absent", () => {
		hasDeletionCapability = false;
		render(<TrackList tracks={[sampleTrack]} />);

		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		expect(screen.queryByText("Delete track")).toBeNull();
	});

	it.each(["Album ensemble", "Taylor Swift"])(
		"shows independent Album Artist details (%s)",
		(albumArtist) => {
			render(
				<TrackList
					tracks={[
						{
							...sampleTrack,
							albumArtists: [
								{ id: "album-artist", name: albumArtist },
								{ id: "second", name: "Guest / Band" },
							],
						},
					]}
				/>,
			);
			fireEvent.contextMenu(
				screen.getByRole("row", { name: /Welcome to New York/ }),
			);
			fireEvent.click(screen.getByText("Details"));
			const dialog = within(
				screen.getByRole("dialog", { name: "Welcome to New York" }),
			);
			expect(dialog.getByText("Artist").parentElement?.textContent).toBe(
				"ArtistTaylor Swift",
			);
			expect(dialog.getByText("Album Artist").parentElement?.textContent).toBe(
				`Album Artist${albumArtist}, Guest / Band`,
			);
		},
	);

	it("renders metadata details and delete actions in the context menu", () => {
		render(<TrackList tracks={[sampleTrack]} />);

		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Details"));

		const dialog = screen.getByRole("dialog", { name: "Welcome to New York" });
		expect(dialog).toBeTruthy();
		expect(dialog.className).toContain("max-w-2xl");
		expect(dialog.className).toContain("bg-popover");
		expect(within(dialog).getByText("Title")).toBeTruthy();
		expect(within(dialog).getByText("Artist")).toBeTruthy();
		expect(within(dialog).queryByText("Album Artist")).toBeNull();
		expect(within(dialog).getByText("Album")).toBeTruthy();
		expect(within(dialog).getByText("Track")).toBeTruthy();
		expect(within(dialog).getByText("Disc")).toBeTruthy();
		expect(within(dialog).getByText("Duration")).toBeTruthy();
		expect(within(dialog).getByText("Codec")).toBeTruthy();
		expect(within(dialog).getByText("Bitrate")).toBeTruthy();
		expect(
			within(dialog).getByText("1856 kbps (Calculated by app)"),
		).toBeTruthy();
		expect(within(dialog).getByText("Sample rate")).toBeTruthy();
		expect(within(dialog).queryByText("Metadata source")).toBeNull();
		expect(within(dialog).getByText("Revision")).toBeTruthy();
		expect(within(dialog).getByText("Bit depth")).toBeTruthy();
		expect(within(dialog).getByText("Genre")).toBeTruthy();
		expect(within(dialog).getByText("Track ReplayGain")).toBeTruthy();
		expect(within(dialog).getByText("Album ReplayGain")).toBeTruthy();
		expect(
			within(dialog).getByText("Gain -7.25 dB · Peak 0.980000"),
		).toBeTruthy();
		expect(
			within(dialog).getByText("Gain -6.50 dB · Peak 1.010000"),
		).toBeTruthy();
		for (const hidden of [
			"Size",
			"Channels",
			"Container",
			"Track total",
			"Disc total",
			"Title sort",
			"Identity key",
			"File path",
			"SHA-256",
			"Album id",
			"Id",
		]) {
			expect(within(dialog).queryByText(hidden)).toBeNull();
		}
		expect(within(dialog).getByText("Taylor Swift")).toBeTruthy();
		expect(within(dialog).getByText("1989")).toBeTruthy();
		expect(within(dialog).getAllByText("1")).toHaveLength(2);
		expect(within(dialog).getByText("3m 32s")).toBeTruthy();
		expect(within(dialog).getByText("flac")).toBeTruthy();
		expect(within(dialog).getByText("96 kHz")).toBeTruthy();
		expect(within(dialog).getByText("24-bit")).toBeTruthy();
		expect(within(dialog).getByText("Pop")).toBeTruthy();
		expect(within(dialog).getByRole("button", { name: "Close" })).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "Close" }));
		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Delete track"));

		const confirmation = screen.getByRole("dialog", {
			name: "Permanently delete Welcome to New York?",
		});
		expect(within(confirmation).getByText("Welcome to New York")).toBeTruthy();
		expect(
			within(confirmation).getByText(
				"library/taylor-swift/1989/01-welcome.flac",
			),
		).toBeTruthy();
		expect(within(confirmation).getByText("47.74 MiB")).toBeTruthy();
		expect(within(confirmation).getByText("Road Trip")).toBeTruthy();
		expect(within(confirmation).getByText("2 Queue items")).toBeTruthy();
		expect(deleteTrack).not.toHaveBeenCalled();

		fireEvent.click(
			within(confirmation).getByRole("button", { name: "Delete permanently" }),
		);
		expect(deleteTrack).toHaveBeenCalledWith(
			{ trackId: "t1", confirmationToken: "confirm-token" },
			expect.any(Object),
		);
	});

	it("returns focus to the Track row after the deletion dialog closes", async () => {
		render(<TrackList tracks={[sampleTrack]} />);
		const row = screen.getByRole("row", { name: /Welcome to New York/ });
		row.focus();

		fireEvent.contextMenu(row);
		fireEvent.click(screen.getByText("Delete track"));
		const dialog = await screen.findByRole("dialog", {
			name: "Permanently delete Welcome to New York?",
		});
		await waitFor(() =>
			expect(dialog.contains(document.activeElement)).toBe(true),
		);

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
		await waitFor(() =>
			expect(
				screen.queryByRole("dialog", {
					name: "Permanently delete Welcome to New York?",
				}),
			).toBeNull(),
		);
		await waitFor(() => expect(document.activeElement).toBe(row));
	});

	it("falls back to the table when the deleted row is gone", async () => {
		const { rerender } = render(<TrackList tracks={[sampleTrack]} />);
		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Delete track"));
		await screen.findByRole("dialog", {
			name: "Permanently delete Welcome to New York?",
		});

		rerender(<TrackList tracks={[]} />);
		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
		await waitFor(() =>
			expect(document.activeElement).toBe(screen.getByRole("table")),
		);
	});

	it("cancels Permanent Track Deletion without mutation", () => {
		render(<TrackList tracks={[sampleTrack]} />);

		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Delete track"));
		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		expect(deleteTrack).not.toHaveBeenCalled();
		expect(
			screen.queryByRole("dialog", {
				name: "Permanently delete Welcome to New York?",
			}),
		).toBeNull();
	});

	it("shows partial ReplayGain Metadata and hides unrecorded ReplayGain rows", () => {
		render(
			<TrackList
				tracks={[
					{
						...sampleTrack,
						replayGain: {
							trackGainDb: -4.25,
							trackPeak: null,
							albumGainDb: null,
							albumPeak: null,
						},
					},
				]}
			/>,
		);

		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		fireEvent.click(screen.getByText("Details"));

		const dialog = screen.getByRole("dialog", { name: "Welcome to New York" });
		expect(within(dialog).getByText("Track ReplayGain")).toBeTruthy();
		expect(within(dialog).getByText("Gain -4.25 dB")).toBeTruthy();
		expect(within(dialog).queryByText("Album ReplayGain")).toBeNull();
		expect(within(dialog).queryByText("Unavailable")).toBeNull();
	});

	it("renders custom remove and delete actions together when supplied", () => {
		const removeTrack = vi.fn();

		render(
			<TrackList
				tracks={[sampleTrack]}
				onRemoveTrack={removeTrack}
				removeLabel="Remove from playlist"
			/>,
		);

		fireEvent.contextMenu(
			screen.getByRole("row", { name: /Welcome to New York/ }),
		);
		expect(screen.getByText("Delete track")).toBeTruthy();
		fireEvent.click(screen.getByText("Remove from playlist"));

		expect(removeTrack).toHaveBeenCalledWith(sampleTrack);
	});
});
