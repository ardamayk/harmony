import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { SidebarPlaylists } from "./sidebar-playlists";

const { getPlaylist, listPlaylists } = vi.hoisted(() => ({
	getPlaylist: vi.fn(),
	listPlaylists: vi.fn(),
}));

vi.mock("#/lib/api", () => ({
	apiClient: {
		getAlbumCoverUrl: (albumId: string) => `/cover/${albumId}`,
		getPlaylist,
		listPlaylists,
	},
}));

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		params,
		title,
	}: {
		children: React.ReactNode;
		params: { playlistId: string };
		title: string;
	}) => (
		<a href={`/playlists/${params.playlistId}`} title={title}>
			{children}
		</a>
	),
}));

function renderList() {
	return render(
		<QueryClientProvider client={new QueryClient()}>
			<SidebarPlaylists />
		</QueryClientProvider>,
	);
}

describe("SidebarPlaylists", () => {
	afterEach(cleanup);

	it("links each Playlist in the server's order", async () => {
		listPlaylists.mockResolvedValue({
			items: [
				{ id: "p1", name: "Road trip", trackCount: 2 },
				{ id: "p2", name: "Late night", trackCount: 0 },
			],
		});
		getPlaylist.mockResolvedValue({
			tracks: [{ albumId: "a1" }, { albumId: "a2" }],
		});
		const { container } = renderList();

		const links = await screen.findAllByRole("link");
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			"/playlists/p1",
			"/playlists/p2",
		]);
		expect(links[0].getAttribute("title")).toBe("Road trip");
		await vi.waitFor(() =>
			expect(
				Array.from(container.querySelectorAll("img")).map((img) =>
					img.getAttribute("src"),
				),
			).toEqual(["/cover/a1"]),
		);
		expect(getPlaylist).toHaveBeenCalledTimes(1);
	});

	it("shows nothing without Playlists", async () => {
		listPlaylists.mockResolvedValue({ items: [] });
		const { container } = renderList();

		await vi.waitFor(() => expect(listPlaylists).toHaveBeenCalled());
		expect(container.querySelector("nav")).toBeNull();
	});
});
