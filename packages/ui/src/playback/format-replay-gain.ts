/** Human-readable ReplayGain summary, or null when nothing was recorded so callers can hide the row. */
export function formatReplayGainAvailability(
	gainDb?: number | null,
	peak?: number | null,
): string | null {
	const details: string[] = [];
	if (gainDb != null) {
		details.push(`Gain ${gainDb.toFixed(2)} dB`);
	}
	if (peak != null) {
		details.push(`Peak ${peak.toFixed(6)}`);
	}
	if (details.length === 0) {
		return null;
	}
	return details.join(" · ");
}
