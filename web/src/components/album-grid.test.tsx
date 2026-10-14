import type { Album } from "@repo/api-client";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";
import { AlbumGrid } from "./album-grid";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		to,
		params: _params,
		...props
	}: {
		children: React.ReactNode;
		to: string;
		params?: Record<string, string>;
	}) => (
		<a href={to} {...props}>
			{children}
		</a>
	),
	useNavigate: () => vi.fn(),
}));

vi.mock("#/hooks/use-server-capability", () => ({
	useServerCapability: () => true,
}));

vi.mock("#/hooks/use-delete-library", () => ({
	useDeleteAlbum: () => ({ mutate: vi.fn(), isPending: false }),
	usePreviewAlbumDeletion: () => ({ mutate: vi.fn(), isPending: false }),
}));

describe("AlbumGrid", () => {
	it("uses the shared collection grid without changing album card behavior", () => {
		const albums = [
			{
				id: "a1",
				title: "Kind of Blue",
				artistName: "Miles Davis",
				trackCount: 5,
				year: 1959,
			},
		] as Album[];

		const { container } = render(<AlbumGrid albums={albums} />);

		const album = screen.getByRole("link", { name: /Kind of Blue/ });
		const grid = container.firstElementChild;
		expect(grid?.className).toContain(
			"grid-cols-[repeat(auto-fill,min(250px,100%))]",
		);
		expect(grid?.className).toContain("gap-3");
		expect(album.className).toContain("aspect-square");
		expect(album.className).toContain("overflow-hidden");
		expect(album.className).toContain("duration-300");
		expect(album.className).toContain("ease-out");
		expect(album.className).toContain("hover:-translate-y-1");
		expect(album.className).toContain("hover:shadow-lg");
		expect(screen.getByText("1959")).toBeTruthy();
		expect(screen.queryByText(/tracks/)).toBeNull();
		expect(container.querySelector("[data-album-card-overlay]")).toBeTruthy();
	});
});
