import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { PlaybackSessionState } from "./PlaybackEngine";
import { InMemoryPlaybackEngine } from "./testing/InMemoryPlaybackEngine";
import { type SleepTimerRequest, useSleepTimer } from "./use-sleep-timer";

function Harness({
	engine,
	now,
}: {
	engine: InMemoryPlaybackEngine;
	now: () => number;
}) {
	const [session, setSession] = React.useState<PlaybackSessionState>(
		engine.getState(),
	);
	React.useEffect(() => {
		const unsubscribe = engine.subscribe(setSession);
		return () => {
			unsubscribe();
		};
	}, [engine]);
	const timer = useSleepTimer(engine, session, now);
	const request = (value: SleepTimerRequest) => () =>
		timer.setSleepTimer(value);
	return (
		<div>
			<span data-testid="mode">{timer.mode.kind}</span>
			<span data-testid="remaining">{timer.remainingSeconds ?? ""}</span>
			<span data-testid="status">{session.status}</span>
			<button type="button" onClick={request({ kind: "after-track" })}>
				After track
			</button>
			<button type="button" onClick={request({ kind: "minutes", minutes: 1 })}>
				One minute
			</button>
			<button type="button" onClick={request({ kind: "off" })}>
				Off
			</button>
		</div>
	);
}

import React from "react";

describe("useSleepTimer", () => {
	afterEach(() => {
		cleanup();
		vi.useRealTimers();
	});

	it("arms the engine's stop-after-current flag for the after-track mode", () => {
		const engine = new InMemoryPlaybackEngine();
		render(<Harness engine={engine} now={Date.now} />);

		act(() => screen.getByRole("button", { name: "After track" }).click());
		expect(engine.getState().stopAfterCurrent).toBe(true);
		expect(screen.getByTestId("mode").textContent).toBe("after-track");

		act(() => screen.getByRole("button", { name: "Off" }).click());
		expect(engine.getState().stopAfterCurrent).toBe(false);
		expect(screen.getByTestId("mode").textContent).toBe("off");
	});

	it("pauses when a minutes timer elapses", async () => {
		vi.useFakeTimers();
		let clock = 1_000_000;
		const now = () => clock;
		const engine = new InMemoryPlaybackEngine();
		await engine.play({
			type: "track",
			track: {
				id: "t",
				title: "T",
				artistName: "A",
				artists: [],
				albumId: "al",
				discNo: 1,
				durationMs: 1000,
				format: "flac",
				genres: [],
			},
			playbackUrl: "/stream/t",
		});
		render(<Harness engine={engine} now={now} />);

		act(() => screen.getByRole("button", { name: "One minute" }).click());
		expect(screen.getByTestId("mode").textContent).toBe("minutes");
		expect(screen.getByTestId("remaining").textContent).toBe("60");

		clock += 30_000;
		act(() => {
			vi.advanceTimersByTime(1000);
		});
		expect(screen.getByTestId("remaining").textContent).toBe("30");
		expect(screen.getByTestId("status").textContent).toBe("playing");

		clock += 30_000;
		act(() => {
			vi.advanceTimersByTime(1000);
		});
		expect(screen.getByTestId("status").textContent).toBe("paused");
		expect(screen.getByTestId("mode").textContent).toBe("off");
	});
});
