import { act, cleanup, render, screen } from "@testing-library/react";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	clearCoverAccentCache,
	nearestTurn,
	useCoverAccent,
	useRootAccentHue,
} from "./use-cover-accent";

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

function HueHarness({
	hue,
	chroma,
	active,
}: {
	hue?: string;
	chroma?: string;
	active: boolean;
}) {
	useRootAccentHue(
		{
			...(hue ? { "--accent-hue": hue } : {}),
			...(chroma ? { "--accent-chroma": chroma } : {}),
		},
		active,
	);
	return null;
}

describe("useRootAccentHue", () => {
	afterEach(() => {
		cleanup();
		document.documentElement.style.removeProperty("--accent-hue");
		document.documentElement.style.removeProperty("--accent-chroma");
		document.documentElement.style.removeProperty("--accent-lightness");
	});

	it("takes the short way round the hue wheel", () => {
		expect(nearestTurn(20, 340)).toBe(380);
		expect(nearestTurn(340, 20)).toBe(-20);
		expect(nearestTurn(200, 62)).toBe(200);
	});

	it("writes the cover hue to the root, the theme hue without one, nothing when inactive", () => {
		const root = document.documentElement;
		// From the peach (62), 340 is nearer one turn down.
		const view = render(<HueHarness hue="340.0" active />);
		expect(root.style.getPropertyValue("--accent-hue")).toBe("-20.0");
		expect(root.style.getPropertyValue("--accent-chroma")).toBe("1");
		expect(root.style.getPropertyValue("--accent-lightness")).toBe("83");
		// A colourless cover: the theme hue, no chroma.
		view.rerender(<HueHarness chroma="0" active />);
		expect(root.style.getPropertyValue("--accent-hue")).toBe("62.0");
		expect(root.style.getPropertyValue("--accent-chroma")).toBe("0");
		view.rerender(<HueHarness hue="120.0" active={false} />);
		expect(root.style.getPropertyValue("--accent-hue")).toBe("");
		expect(root.style.getPropertyValue("--accent-chroma")).toBe("");
	});
});
