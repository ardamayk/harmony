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
	clearTrackWaveformCache,
	type TrackWaveformLoader,
	type TrackWaveformResult,
	useTrackWaveform,
} from "./use-track-waveform";

function Harness({
	trackId,
	enabled,
	load,
}: {
	trackId: string | null;
	enabled: boolean;
	load: TrackWaveformLoader | null;
}) {
	const peaks = useTrackWaveform({ trackId, enabled, load });
	return <span data-testid="peaks">{peaks ? peaks.join(",") : "none"}</span>;
}

describe("useTrackWaveform", () => {
	beforeEach(clearTrackWaveformCache);
	afterEach(() => {
		cleanup();
		vi.useRealTimers();
	});

	it("polls while pending, then caches the peaks per track", async () => {
		vi.useFakeTimers();
		const load = vi
			.fn<TrackWaveformLoader>()
			.mockResolvedValueOnce({ status: "pending", retryAfterSeconds: 2 })
			.mockResolvedValueOnce({
				trackId: "t1",
				peakCount: 3,
				peaks: [0, 128, 255],
			});
		const view = render(<Harness trackId="t1" enabled load={load} />);
		await act(async () => {});
		expect(screen.getByTestId("peaks").textContent).toBe("none");

		await act(async () => {
			vi.advanceTimersByTime(2000);
		});
		await act(async () => {});
		expect(screen.getByTestId("peaks").textContent).toBe("0,128,255");
		expect(load).toHaveBeenCalledTimes(2);

		view.rerender(<Harness trackId="t1" enabled load={load} />);
		expect(load).toHaveBeenCalledTimes(2);
	});

	it("reloads a mounted track after same-id replacement invalidates the cache", async () => {
		const load = vi.fn<TrackWaveformLoader>().mockResolvedValue({
			trackId: "t1",
			peakCount: 2,
			peaks: [10, 20],
		});
		render(<Harness trackId="t1" enabled load={load} />);
		await act(async () => {});
		expect(screen.getByTestId("peaks").textContent).toBe("10,20");

		load.mockResolvedValue({ trackId: "t1", peakCount: 2, peaks: [30, 40] });
		await act(async () => clearTrackWaveformCache());

		expect(screen.getByTestId("peaks").textContent).toBe("30,40");
		expect(load).toHaveBeenCalledTimes(2);
	});

	it("revalidates cached peaks on remount for replacements from another client", async () => {
		const load = vi.fn<TrackWaveformLoader>().mockResolvedValue({
			trackId: "t1",
			peakCount: 1,
			peaks: [10],
		});
		const view = render(<Harness trackId="t1" enabled load={load} />);
		await act(async () => {});
		view.unmount();
		load.mockResolvedValue({ trackId: "t1", peakCount: 1, peaks: [20] });

		render(<Harness trackId="t1" enabled load={load} />);
		expect(screen.getByTestId("peaks").textContent).toBe("10");
		await act(async () => {});
		expect(screen.getByTestId("peaks").textContent).toBe("20");
		expect(load).toHaveBeenCalledTimes(2);
	});

	it("discards pre-invalidation responses without repopulating the cache", async () => {
		let resolveOld: (result: TrackWaveformResult) => void = () => undefined;
		const load = vi
			.fn<TrackWaveformLoader>()
			.mockImplementationOnce(
				() =>
					new Promise((resolve) => {
						resolveOld = resolve;
					}),
			)
			.mockResolvedValue({ trackId: "t1", peakCount: 1, peaks: [20] });
		const view = render(<Harness trackId="t1" enabled load={load} />);
		await act(async () => clearTrackWaveformCache());
		expect(screen.getByTestId("peaks").textContent).toBe("20");

		await act(async () => {
			resolveOld({ trackId: "t1", peakCount: 1, peaks: [10] });
		});
		expect(screen.getByTestId("peaks").textContent).toBe("20");
		view.unmount();
		load.mockImplementation(() => new Promise(() => {}));
		render(<Harness trackId="t1" enabled load={load} />);
		expect(screen.getByTestId("peaks").textContent).toBe("20");
	});

	it("cancels pending polls when the cache is invalidated", async () => {
		vi.useFakeTimers();
		const load = vi
			.fn<TrackWaveformLoader>()
			.mockResolvedValueOnce({ status: "pending", retryAfterSeconds: 2 })
			.mockResolvedValue({ trackId: "t1", peakCount: 1, peaks: [20] });
		render(<Harness trackId="t1" enabled load={load} />);
		await act(async () => {});
		await act(async () => clearTrackWaveformCache());
		await act(async () => vi.advanceTimersByTime(2000));

		expect(screen.getByTestId("peaks").textContent).toBe("20");
		expect(load).toHaveBeenCalledTimes(2);
	});

	it("stays empty when disabled or when the server has no waveform", async () => {
		const failing = vi.fn<TrackWaveformLoader>(async () => {
			throw new Error("503");
		});
		render(<Harness trackId="t2" enabled load={failing} />);
		await act(async () => {});
		expect(screen.getByTestId("peaks").textContent).toBe("none");

		const load = vi.fn<TrackWaveformLoader>();
		render(<Harness trackId="t3" enabled={false} load={load} />);
		expect(load).not.toHaveBeenCalled();
	});
});
