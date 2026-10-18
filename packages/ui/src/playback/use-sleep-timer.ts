import { useCallback, useEffect, useState } from "react";
import type { PlaybackEngine, PlaybackSessionState } from "./PlaybackEngine";

export type SleepTimerMode =
	| { kind: "off" }
	| { kind: "after-track" }
	| { kind: "minutes"; minutes: number; endsAt: number };

export type SleepTimerRequest =
	| { kind: "off" }
	| { kind: "after-track" }
	| { kind: "minutes"; minutes: number };

export type SleepTimer = {
	mode: SleepTimerMode;
	/** Seconds left for a minutes timer; null otherwise. */
	remainingSeconds: number | null;
	setSleepTimer: (request: SleepTimerRequest) => void;
};

const TICK_MS = 1000;

/**
 * Sleep timer above the engine. "After this track" is delegated to the
 * engine's stop-after-current flag so a native engine that owns queue
 * advancement honours it; a minutes timer simply pauses when it elapses.
 */
export function useSleepTimer(
	engine: PlaybackEngine,
	session: PlaybackSessionState,
	now: () => number = Date.now,
): SleepTimer {
	const [minutesTimer, setMinutesTimer] = useState<{
		minutes: number;
		endsAt: number;
	} | null>(null);
	// When the minutes timer was last checked; remaining time derives from it.
	const [checkedAt, setCheckedAt] = useState(now);

	useEffect(() => {
		if (!minutesTimer) return undefined;
		const interval = window.setInterval(() => {
			const checked = now();
			setCheckedAt(checked);
			if (checked >= minutesTimer.endsAt) {
				engine.pause();
				setMinutesTimer(null);
			}
		}, TICK_MS);
		return () => window.clearInterval(interval);
	}, [engine, minutesTimer, now]);

	const setSleepTimer = useCallback(
		(request: SleepTimerRequest) => {
			setMinutesTimer(null);
			engine.setStopAfterCurrent?.(request.kind === "after-track");
			if (request.kind === "minutes") {
				const startedAt = now();
				setCheckedAt(startedAt);
				setMinutesTimer({
					minutes: request.minutes,
					endsAt: startedAt + request.minutes * 60_000,
				});
			}
		},
		[engine, now],
	);

	const remainingSeconds = minutesTimer
		? Math.max(0, Math.ceil((minutesTimer.endsAt - checkedAt) / 1000))
		: null;

	const mode: SleepTimerMode = minutesTimer
		? { kind: "minutes", ...minutesTimer }
		: session.stopAfterCurrent
			? { kind: "after-track" }
			: { kind: "off" };

	return { mode, remainingSeconds, setSleepTimer };
}
