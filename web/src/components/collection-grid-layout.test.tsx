import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
	CollectionGrid,
	CollectionGridSkeleton,
	CollectionGridState,
	CollectionPageContainer,
} from "./collection-grid-layout";

afterEach(cleanup);

describe("collection grid layout", () => {
	it("shares the playlist width and responsive grid across collection pages", () => {
		render(
			<CollectionPageContainer data-testid="collection-container">
				<CollectionGrid isBusy>
					<div>Card</div>
				</CollectionGrid>
			</CollectionPageContainer>,
		);

		const container = screen.getByTestId("collection-container");
		expect(container.className).toContain("w-full");
		expect(container.className).toContain("page-content-column");

		const grid = screen.getByText("Card").parentElement;
		expect(grid?.className).toContain(
			"grid-cols-[repeat(auto-fill,min(250px,100%))]",
		);
		expect(grid?.className).toContain("gap-3");
		expect(grid?.className).toContain(
			"max-lg:in-data-[queue-open]:grid-cols-2",
		);
		expect(grid?.getAttribute("aria-busy")).toBe("true");
	});

	it("announces loading while rendering ten decorative square skeletons", () => {
		render(<CollectionGridSkeleton label="Loading albums" />);

		expect(screen.getByRole("status").textContent).toContain("Loading albums");
		const skeletons = screen.getAllByTestId("collection-card-skeleton");
		expect(skeletons).toHaveLength(10);
		for (const skeleton of skeletons) {
			expect(skeleton.className).toContain("aspect-square");
			expect(skeleton.getAttribute("aria-hidden")).toBe("true");
		}
	});

	it("announces collection errors and retries on request", () => {
		const handleRetry = vi.fn();
		render(
			<CollectionGridState
				kind="error"
				icon={<svg aria-hidden />}
				title="Unable to load albums"
				description="Check your connection and try again."
				onRetry={handleRetry}
			/>,
		);

		expect(screen.getByRole("alert").textContent).toContain(
			"Unable to load albums",
		);
		fireEvent.click(screen.getByRole("button", { name: "Try again" }));
		expect(handleRetry).toHaveBeenCalledOnce();
	});

	it("disables the retry action while a collection refetch is running", () => {
		render(
			<CollectionGridState
				kind="error"
				icon={<svg aria-hidden />}
				title="Unable to load artists"
				description="Check your connection and try again."
				onRetry={vi.fn()}
				isRetrying
			/>,
		);

		const button = screen.getByRole("button", { name: "Trying again…" });
		expect((button as HTMLButtonElement).disabled).toBe(true);
	});

	it("renders collection empty states without inventing an action", () => {
		render(
			<CollectionGridState
				kind="empty"
				icon={<svg aria-hidden />}
				title="No playlists yet"
				description="Create a playlist to organize your library."
			/>,
		);

		expect(screen.getByText("No playlists yet")).toBeTruthy();
		expect(screen.queryByRole("button")).toBeNull();
	});
});
