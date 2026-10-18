import { act, cleanup, render, screen } from "@testing-library/react";
import { useEffect, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { PlaybackSessionState, PlaybackSource } from "./PlaybackEngine";
import { InMemoryPlaybackEngine } from "./testing/InMemoryPlaybackEngine";
import { useAbRepeat } from "./use-ab-repeat";

function source(id: string): PlaybackSource {
	return {
		type: "track",
		track: {
			id,
			title: id,
			artistName: "A",
			artists: [],
			albumId: "al",
			discNo: 1,
			durationMs: 100_000,
			format: "flac",
			genres: [],
		},
		playbackUrl: `/stream/${id}`,
	};
}

function Harness({ engine }: { engine: InMemoryPlaybackEngine }) {
	const [session, setSession] = useState<PlaybackSessionState>(
		engine.getState(),
	);
	useEffect(() => {
		const unsubscribe = engine.subscribe(setSession);
		return () => {
			unsubscribe();
		};
	}, [engine]);
	const ab = useAbRepeat(engine, session);
	return (
		<div>
			<span data-testid="points">{`${ab.a ?? "-"}/${ab.b ?? "-"}`}</span>
			<span data-testid="active">{String(ab.isActive)}</span>
			<button type="button" onClick={() => ab.setPoint("a")}>
				A
			</button>
			<button type="button" onClick={() => ab.setPoint("b")}>
				B
			</button>
			<button type="button" onClick={ab.clear}>
				Clear
			</button>
		</div>
	);
}

describe("useAbRepeat", () => {
	afterEach(cleanup);

	it("loops back to A when the playhead passes B and clears on a new Track", async () => {
		const engine = new InMemoryPlaybackEngine();
		await engine.play(source("one"));
		const seek = vi.spyOn(engine, "seek");
		render(<Harness engine={engine} />);

		act(() => engine.seek(10));
		act(() => screen.getByRole("button", { name: "A" }).click());
		act(() => engine.seek(20));
		act(() => screen.getByRole("button", { name: "B" }).click());
		expect(screen.getByTestId("points").textContent).toBe("10/20");
		expect(screen.getByTestId("active").textContent).toBe("true");

		seek.mockClear();
		act(() => engine.seek(21));
		expect(seek).toHaveBeenLastCalledWith(10);

		await act(async () => {
			await engine.play(source("two"));
		});
		expect(screen.getByTestId("points").textContent).toBe("-/-");
	});

	it("drops B when A is moved past it", async () => {
		const engine = new InMemoryPlaybackEngine();
		await engine.play(source("one"));
		render(<Harness engine={engine} />);

		act(() => engine.seek(5));
		act(() => screen.getByRole("button", { name: "A" }).click());
		act(() => engine.seek(15));
		act(() => screen.getByRole("button", { name: "B" }).click());
		// While playing, the loop would pull the playhead back before A moves.
		act(() => engine.pause());
		act(() => engine.seek(30));
		act(() => screen.getByRole("button", { name: "A" }).click());
		expect(screen.getByTestId("points").textContent).toBe("30/-");
		expect(screen.getByTestId("active").textContent).toBe("false");

		act(() => screen.getByRole("button", { name: "Clear" }).click());
		expect(screen.getByTestId("points").textContent).toBe("-/-");
	});
});
