import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { SidebarPlaylists } from "./sidebar-playlists";

const listPlaylists = vi.hoisted(() => vi.fn());

vi.mock("#/lib/api", () => ({ apiClient: { listPlaylists } }));

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
				{ id: "p1", name: "Road trip" },
				{ id: "p2", name: "Late night" },
			],
		});
		renderList();

		const links = await screen.findAllByRole("link");
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			"/playlists/p1",
			"/playlists/p2",
		]);
		expect(links[0].getAttribute("title")).toBe("Road trip");
	});

	it("shows nothing without Playlists", async () => {
		listPlaylists.mockResolvedValue({ items: [] });
		const { container } = renderList();

		await vi.waitFor(() => expect(listPlaylists).toHaveBeenCalled());
		expect(container.querySelector("nav")).toBeNull();
	});
});
