export type LyricLine = { text: string; time?: number };

/** Lyrics split into lines; `synced` when they carry LRC timestamps. */
export type ParsedLyrics = { synced: boolean; lines: LyricLine[] };

const TIMESTAMP = /\[(\d+):(\d+(?:\.\d+)?)\]/g;
const OFFSET = /^\[offset:\s*([+-]?\d+)\s*\]$/im;

/**
 * Splits stored lyrics into lines. LRC text (`[mm:ss.xx] words`) becomes
 * timed lines sorted by time; plain text keeps its lines with runs of blank
 * lines collapsed. Null when there is nothing to show.
 */
export function parseLyrics(text: string): ParsedLyrics | null {
	const rows = text.split(/\r?\n/).map((row) => row.trimEnd());
	// LRC's `[offset:ms]`: a positive offset shows every line that much sooner.
	const offset = Number(text.match(OFFSET)?.[1] ?? 0) / 1000;
	const timed: LyricLine[] = [];
	for (const row of rows) {
		const times = [...row.matchAll(TIMESTAMP)].map(
			(match) => Number(match[1]) * 60 + Number(match[2]),
		);
		const words = row.replace(TIMESTAMP, "").trim();
		// One line may carry several timestamps when a chorus repeats.
		for (const time of times)
			timed.push({ time: Math.max(0, time - offset), text: words });
	}
	if (timed.length > 0) {
		timed.sort((a, b) => (a.time ?? 0) - (b.time ?? 0));
		return { synced: true, lines: timed };
	}
	const lines = rows
		.filter((row, index) => row !== "" || rows[index - 1] !== "")
		.map((row) => ({ text: row }));
	return lines.every((line) => line.text === "")
		? null
		: { synced: false, lines };
}

/** Index of the line sung at `seconds`; -1 before the first line starts. */
export function activeLyricIndex(lines: LyricLine[], seconds: number): number {
	let active = -1;
	for (const [index, line] of lines.entries()) {
		if ((line.time ?? Number.POSITIVE_INFINITY) > seconds) break;
		active = index;
	}
	return active;
}
