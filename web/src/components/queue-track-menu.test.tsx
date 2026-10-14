import type { QueueItem } from "@repo/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { QueueTrackMenu } from "./queue-track-menu";

const mocks = vi.hoisted(() => ({
	playNext: vi.fn(),
	removeFromQueue: vi.fn(),
	listPlaylists: vi.fn(),
	addPlaylistTrack: vi.fn(),
	navigate: vi.fn(),
	toastError: vi.fn(),
	toastSuccess: vi.fn(),
	playRow: vi.fn(),
}));
vi.mock("@repo/ui", async (importOriginal) => ({
	...(await importOriginal<typeof import("@repo/ui")>()),
	usePlayback: () => mocks,
	usePlaylistLibrary: () => mocks,
	toast: { error: mocks.toastError, success: mocks.toastSuccess },
}));
vi.mock("@tanstack/react-router", () => ({
	useNavigate: () => mocks.navigate,
}));

const item: QueueItem = {
	id: "queue-second-copy",
	trackId: "track-1",
	position: 0,
	track: {
		id: "track-1",
		title: "Track one",
		artistName: "Artist one",
		artists: [{ id: "artist-1", name: "Artist one" }],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120000,
		format: "flac",
		genres: [],
	},
};

function renderMenu(menuItem = item) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	render(
		<QueryClientProvider client={queryClient}>
			<ul>
				<QueueTrackMenu item={menuItem}>
					{(trigger) => (
						// biome-ignore lint/a11y/useSemanticElements: Reproduce the host's focusable queue row contract.
						<li
							// biome-ignore lint/a11y/noNoninteractiveElementToInteractiveRole: Reproduce the host's focusable queue row contract.
							role="button"
							tabIndex={0}
							onClick={mocks.playRow}
							onKeyDown={(event) => {
								if (event.key === "Enter" || event.key === " ") mocks.playRow();
							}}
						>
							{item.track.title}
							{trigger}
						</li>
					)}
				</QueueTrackMenu>
			</ul>
		</QueryClientProvider>,
	);
	return queryClient;
}

function openDropdown() {
	const trigger = screen.getByRole("button", {
		name: "Queue actions for Track one",
	});
	trigger.focus();
	fireEvent.keyDown(trigger, { key: "Enter" });
}

async function openPlaylists() {
	const submenu = screen.getByRole("menuitem", { name: "Add to playlist" });
	submenu.focus();
	fireEvent.keyDown(submenu, { key: "ArrowRight" });
}

beforeEach(() => {
	vi.resetAllMocks();
	mocks.listPlaylists.mockResolvedValue({
		items: [{ id: "playlist-1", name: "Favorites" }],
	});
	mocks.addPlaylistTrack.mockResolvedValue({});
	mocks.playNext.mockResolvedValue(undefined);
	mocks.removeFromQueue.mockResolvedValue(undefined);
	mocks.navigate.mockResolvedValue(undefined);
});
afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("QueueTrackMenu", () => {
	it("opens the visible trigger with the keyboard and plays next without playing the row", async () => {
		renderMenu();
		openDropdown();
		const action = await screen.findByRole("menuitem", { name: "Play next" });
		action.focus();
		fireEvent.keyDown(action, { key: "Enter" });
		expect(mocks.playNext).toHaveBeenCalledWith("track-1");
		expect(mocks.playRow).not.toHaveBeenCalled();
	});

	it.each(["pointer", "F10", "ContextMenu"])(
		"opens the row context menu via %s and removes the queue item",
		async (method) => {
			renderMenu();
			const row = document.querySelector("li");
			if (!row) throw new Error("Missing queue row");
			row.focus();
			if (method === "pointer") fireEvent.contextMenu(row);
			else fireEvent.keyDown(row, { key: method, shiftKey: method === "F10" });
			fireEvent.click(
				await screen.findByRole("menuitem", { name: "Remove from queue" }),
			);
			expect(mocks.removeFromQueue).toHaveBeenCalledWith("queue-second-copy");
			expect(mocks.playRow).not.toHaveBeenCalled();
		},
	);

	it.each(["pointer", " "])(
		"opens the visible trigger via %s without playing the row",
		async (method) => {
			renderMenu();
			const trigger = screen.getByRole("button", {
				name: "Queue actions for Track one",
			});
			if (method === "pointer") {
				fireEvent(
					trigger,
					new MouseEvent("pointerdown", { bubbles: true, button: 0 }),
				);
				fireEvent.click(trigger);
			} else {
				trigger.focus();
				fireEvent.keyDown(trigger, { key: method });
			}
			expect(
				await screen.findByRole("menuitem", { name: "Play next" }),
			).toBeTruthy();
			expect(mocks.playRow).not.toHaveBeenCalled();
		},
	);

	it("adds to a playlist and invalidates host playlist queries", async () => {
		const queryClient = renderMenu();
		const invalidate = vi.spyOn(queryClient, "invalidateQueries");
		openDropdown();
		await openPlaylists();
		fireEvent.click(await screen.findByRole("menuitem", { name: "Favorites" }));
		await waitFor(() =>
			expect(mocks.toastSuccess).toHaveBeenCalledWith("Added to Favorites"),
		);
		expect(mocks.addPlaylistTrack).toHaveBeenCalledWith(
			"playlist-1",
			"track-1",
		);
		expect(invalidate).toHaveBeenCalledWith({ queryKey: ["playlists"] });
		expect(invalidate).toHaveBeenCalledWith({
			queryKey: ["playlist", "playlist-1"],
		});
		expect(mocks.playRow).not.toHaveBeenCalled();
	});

	it.each([
		[
			"Go to album",
			{ to: "/library/$albumId", params: { albumId: "album-1" } },
		],
		[
			"Go to artist",
			{ to: "/library/tracks", search: { artistId: "artist-1" } },
		],
	])("navigates using %s", async (label, destination) => {
		renderMenu();
		openDropdown();
		fireEvent.click(await screen.findByRole("menuitem", { name: label }));
		expect(mocks.navigate).toHaveBeenCalledWith(destination);
		expect(mocks.playRow).not.toHaveBeenCalled();
	});

	it.each(["dropdown", "context"])(
		"chooses an exact credit from the %s artist menu",
		async (mode) => {
			renderMenu({
				...item,
				track: {
					...item.track,
					artistName: "Artist one & Guest / Band",
					artists: [
						...item.track.artists,
						{ id: "guest-id", name: "Guest / Band" },
					],
				},
			});
			if (mode === "dropdown") openDropdown();
			else
				fireEvent.contextMenu(
					screen.getByRole("button", { name: "Track one" }),
				);
			const trigger = await screen.findByRole("menuitem", {
				name: "Go to artist",
			});
			trigger.focus();
			fireEvent.keyDown(trigger, { key: "ArrowRight" });
			expect(mocks.navigate).not.toHaveBeenCalled();
			expect(
				await screen.findByRole("menuitem", { name: "Artist one" }),
			).toBeTruthy();
			const guest = await screen.findByRole("menuitem", {
				name: "Guest / Band",
			});
			guest.focus();
			fireEvent.keyDown(guest, { key: "Enter" });
			expect(mocks.navigate).toHaveBeenCalledWith({
				to: "/library/tracks",
				search: { artistId: "guest-id" },
			});
			expect(mocks.playRow).not.toHaveBeenCalled();
		},
	);

	it.each(
		[
			undefined,
			[],
			[{ id: "", name: "Artist one" }],
			[{ id: "legacy-artist:Artist one", name: "Artist one" }],
		].map((artists) => ({ artists })),
	)(
		"disables artist navigation without real credit IDs ($artists)",
		async ({ artists }) => {
			renderMenu({ ...item, track: { ...item.track, artists } } as QueueItem);
			openDropdown();
			const action = await screen.findByRole("menuitem", {
				name: "Go to artist",
			});
			expect(action.getAttribute("aria-disabled")).toBe("true");
			fireEvent.click(action);
			expect(mocks.navigate).not.toHaveBeenCalled();
		},
	);

	it("reports action failures", async () => {
		const error = new Error("queue unavailable");
		mocks.playNext.mockRejectedValue(error);
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		renderMenu();
		openDropdown();
		fireEvent.click(await screen.findByRole("menuitem", { name: "Play next" }));
		await waitFor(() =>
			expect(mocks.toastError).toHaveBeenCalledWith(
				"Failed to play track next",
			),
		);
		expect(warn).toHaveBeenCalledWith(
			"Failed to play track next",
			expect.objectContaining({ error, queueItemId: item.id }),
		);
	});

	it("reports playlist load failures and offers retry", async () => {
		mocks.listPlaylists.mockRejectedValueOnce(new Error("offline"));
		vi.spyOn(console, "warn").mockImplementation(() => {});
		renderMenu();
		openDropdown();
		await openPlaylists();
		fireEvent.click(
			await screen.findByRole("menuitem", { name: "Retry loading playlists" }),
		);
		expect(
			await screen.findByRole("menuitem", { name: "Favorites" }),
		).toBeTruthy();
		expect(mocks.toastError).toHaveBeenCalledWith("Failed to load playlists");
	});
});
