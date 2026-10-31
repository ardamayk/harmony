import { isDesktopClient } from "#/desktop/bridge";
import { apiClient } from "#/lib/api";

type Metric = { name: string; value: number };

const REPORT_INTERVAL_MS = 60_000;
/** The Music Server's limit for one report. */
const MAX_METRICS_PER_REPORT = 32;
/** Ignores the many interactions too short to feel slow. */
const EVENT_DURATION_THRESHOLD_MS = 40;

let pending: Metric[] = [];
let isRefused = false;

export function recordClientMetric(name: string, value: number) {
	if (!isRefused) pending.push({ name, value });
}

function reportPending() {
	const metrics = pending.slice(0, MAX_METRICS_PER_REPORT);
	pending = pending.slice(MAX_METRICS_PER_REPORT);
	if (metrics.length === 0) return;
	apiClient
		.reportClientMetrics({
			client: isDesktopClient() ? "desktop" : "web",
			metrics,
		})
		.catch(() => {
			// A Guest, or an older Music Server: measuring is not worth retrying.
			isRefused = true;
			pending = [];
		});
}

function observe(
	type: string,
	onEntries: (entries: PerformanceEntry[]) => void,
	options: PerformanceObserverInit = {},
) {
	if (!PerformanceObserver.supportedEntryTypes?.includes(type)) return;
	new PerformanceObserver((list) => onEntries(list.getEntries())).observe({
		type,
		buffered: true,
		...options,
	});
}

/** The largest value of each page metric so far, reported once. */
export function largestEventDuration(
	entries: readonly { duration: number }[],
	largest: number,
): number {
	return entries.reduce(
		(most, entry) => Math.max(most, entry.duration),
		largest,
	);
}

/** Measures the page in the browser's own terms, with no third party. */
export function startClientMetrics() {
	if (typeof PerformanceObserver === "undefined") return;
	let contentful = 0;
	let largestPaint = 0;
	let layoutShift = 0;
	let interaction = 0;

	observe("paint", (entries) => {
		for (const entry of entries) {
			if (entry.name === "first-contentful-paint") contentful = entry.startTime;
		}
	});
	observe("largest-contentful-paint", (entries) => {
		largestPaint = entries.at(-1)?.startTime ?? largestPaint;
	});
	observe("layout-shift", (entries) => {
		for (const entry of entries as (PerformanceEntry & {
			hadRecentInput: boolean;
			value: number;
		})[]) {
			if (!entry.hadRecentInput) layoutShift += entry.value;
		}
	});
	observe(
		"event",
		(entries) => {
			interaction = largestEventDuration(entries, interaction);
		},
		{
			durationThreshold: EVENT_DURATION_THRESHOLD_MS,
		} as PerformanceObserverInit,
	);

	let hasReportedPage = false;
	const reportPage = () => {
		if (!hasReportedPage) {
			hasReportedPage = true;
			const navigation = performance.getEntriesByType("navigation")[0] as
				| PerformanceNavigationTiming
				| undefined;
			if (navigation) {
				recordClientMetric("client.page.ttfb", navigation.responseStart);
			}
			if (contentful > 0) recordClientMetric("client.page.fcp", contentful);
			if (largestPaint > 0) recordClientMetric("client.page.lcp", largestPaint);
			recordClientMetric("client.page.cls", layoutShift);
		}
		if (interaction > 0) {
			recordClientMetric("client.interaction.inp", interaction);
			interaction = 0;
		}
		reportPending();
	};
	document.addEventListener("visibilitychange", () => {
		if (document.visibilityState === "hidden") reportPage();
	});
	window.addEventListener("pagehide", reportPage);
	setInterval(reportPage, REPORT_INTERVAL_MS);
}
