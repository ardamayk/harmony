import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const reportClientMetrics = vi.hoisted(() => vi.fn());

vi.mock("#/lib/api", () => ({ apiClient: { reportClientMetrics } }));
vi.mock("#/desktop/bridge", () => ({ isDesktopClient: () => true }));

describe("client metrics", () => {
	beforeEach(() => {
		vi.resetModules();
		reportClientMetrics.mockReset();
		reportClientMetrics.mockResolvedValue(undefined);
	});

	it("keeps the longest interaction", async () => {
		const { largestEventDuration } = await import("./client-metrics");

		expect(
			largestEventDuration([{ duration: 40 }, { duration: 120 }], 80),
		).toBe(120);
		expect(largestEventDuration([{ duration: 40 }], 80)).toBe(80);
	});

	it("reports what was recorded when the page hides, once", async () => {
		const { recordClientMetric, startClientMetrics } =
			await import("./client-metrics");
		vi.stubGlobal(
			"PerformanceObserver",
			class {
				static supportedEntryTypes: string[] = [];
			},
		);
		startClientMetrics();
		recordClientMetric("client.playback.start", 120);

		window.dispatchEvent(new Event("pagehide"));
		window.dispatchEvent(new Event("pagehide"));

		expect(reportClientMetrics).toHaveBeenCalledTimes(1);
		expect(reportClientMetrics.mock.calls[0][0].client).toBe("desktop");
		expect(reportClientMetrics.mock.calls[0][0].metrics).toContainEqual({
			name: "client.playback.start",
			value: 120,
		});
		vi.unstubAllGlobals();
	});

	it("stops measuring once the server refuses", async () => {
		reportClientMetrics.mockRejectedValue(new Error("not found"));
		const { recordClientMetric, startClientMetrics } =
			await import("./client-metrics");
		vi.stubGlobal(
			"PerformanceObserver",
			class {
				static supportedEntryTypes: string[] = [];
			},
		);
		startClientMetrics();
		window.dispatchEvent(new Event("pagehide"));
		await vi.waitFor(() => expect(reportClientMetrics).toHaveBeenCalledOnce());
		await Promise.resolve();

		recordClientMetric("client.playback.start", 120);
		window.dispatchEvent(new Event("pagehide"));

		expect(reportClientMetrics).toHaveBeenCalledOnce();
		vi.unstubAllGlobals();
	});
});
