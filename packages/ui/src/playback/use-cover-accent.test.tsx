import { act, cleanup, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import { clearCoverAccentCache, useCoverAccent } from "./use-cover-accent";

vi.mock("../lib/cover-accent", async (importOriginal) => {
	const original = await importOriginal<typeof import("../lib/cover-accent")>();
	return {
		...original,
		extractAccent: vi.fn(() => ({ h: 120, s: 0.6, l: 0.6 })),
	};
});

function Harness({
	cacheKey,
	enabled,
	loadImage,
}: {
	cacheKey: string;
	enabled: boolean;
	loadImage: (url: string) => Promise<HTMLImageElement>;
}) {
	const style = useCoverAccent({
		cacheKey,
		coverUrl: `/cover/${cacheKey}`,
		enabled,
		mode: "dark",
		loadImage,
	});
	return <span data-testid="accent">{style["--player-accent"] ?? ""}</span>;
}

describe("useCoverAccent", () => {
	beforeEach(clearCoverAccentCache);
	afterEach(cleanup);

	it("extracts once per album and reuses the cached accent", async () => {
		const loadImage = vi.fn(async () => new Image());
		const view = render(
			<Harness cacheKey="album-1" enabled loadImage={loadImage} />,
		);
		await act(async () => {});
		expect(screen.getByTestId("accent").textContent).toBe("hsl(120 60% 60%)");
		expect(loadImage).toHaveBeenCalledTimes(1);

		view.rerender(<Harness cacheKey="album-1" enabled loadImage={loadImage} />);
		await act(async () => {});
		expect(loadImage).toHaveBeenCalledTimes(1);
	});

	it("returns nothing while disabled or when the cover fails to load", async () => {
		const failing = vi.fn(async () => {
			throw new Error("nope");
		});
		render(<Harness cacheKey="album-2" enabled loadImage={failing} />);
		await act(async () => {});
		expect(screen.getByTestId("accent").textContent).toBe("");

		render(<Harness cacheKey="album-3" enabled={false} loadImage={vi.fn()} />);
		expect(screen.getAllByTestId("accent")[1]?.textContent).toBe("");
	});
});
