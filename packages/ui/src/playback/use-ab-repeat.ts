import { useCallback, useEffect, useRef, useState } from "react";
import type { PlaybackEngine, PlaybackSessionState } from "./PlaybackEngine";

export type AbRepeat = {
	a: number | null;
	b: number | null;
	/** Both points set and ordered, so the loop is active. */
	isActive: boolean;
	setPoint: (point: "a" | "b") => void;
	clear: () => void;
};

/**
 * A-B repeat: loops between two positions of the current Track by seeking
 * back to A whenever the playhead passes B. Precision is bounded by how
 * often the engine reports time, which is fine for practice loops.
 */
export function useAbRepeat(
	engine: PlaybackEngine,
	session: PlaybackSessionState,
): AbRepeat {
	const [points, setPoints] = useState<{ a: number | null; b: number | null }>({
		a: null,
		b: null,
	});
	const trackId =
		session.source?.type === "track" ? session.source.track.id : null;
	const lastTrackIdRef = useRef(trackId);

	useEffect(() => {
		if (lastTrackIdRef.current === trackId) return;
		lastTrackIdRef.current = trackId;
		setPoints({ a: null, b: null });
	}, [trackId]);

	const isActive =
		points.a !== null && points.b !== null && points.b > points.a;

	useEffect(() => {
		if (!isActive || points.a === null || points.b === null) return;
		if (session.status !== "playing") return;
		if (session.currentTime >= points.b) engine.seek(points.a);
	}, [engine, isActive, points, session.currentTime, session.status]);

	const setPoint = useCallback(
		(point: "a" | "b") => {
			const at = session.currentTime;
			setPoints((current) => {
				if (point === "a") {
					// Moving A past B invalidates B.
					return {
						a: at,
						b: current.b !== null && current.b > at ? current.b : null,
					};
				}
				return { a: current.a, b: at };
			});
		},
		[session.currentTime],
	);

	const clear = useCallback(() => setPoints({ a: null, b: null }), []);

	return { a: points.a, b: points.b, isActive, setPoint, clear };
}
