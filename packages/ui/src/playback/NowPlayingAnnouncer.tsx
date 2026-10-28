import { useEffect, useState } from "react";
import { usePlayback } from "./PlaybackProvider";

const ANNOUNCE_DELAY_MS = 400;

/**
 * Visually hidden live region so screen readers hear track changes without
 * the bar itself being a live region. Radio metadata churn is ignored: only
 * a new track or a new station is announced.
 */
export function NowPlayingAnnouncer() {
	const { currentTrack, currentRadioStation, currentExternalTrack } =
		usePlayback();
	const [message, setMessage] = useState("");

	const sourceKey = currentTrack
		? `track:${currentTrack.id}`
		: currentExternalTrack
			? `external:${currentExternalTrack.id}`
			: currentRadioStation
				? `station:${currentRadioStation.id}`
				: null;
	const nextMessage = currentTrack
		? `Now playing: ${currentTrack.title} by ${currentTrack.artistName}`
		: currentExternalTrack
			? currentExternalTrack.artistName
				? `Now playing: ${currentExternalTrack.title} by ${currentExternalTrack.artistName}`
				: `Now playing: ${currentExternalTrack.title}`
			: currentRadioStation
				? `Now playing: ${currentRadioStation.name}`
				: "";

	if (!sourceKey && message !== "") setMessage("");

	useEffect(() => {
		if (!sourceKey) return undefined;
		const timer = window.setTimeout(
			() => setMessage(nextMessage),
			ANNOUNCE_DELAY_MS,
		);
		return () => window.clearTimeout(timer);
	}, [sourceKey, nextMessage]);

	return (
		<div
			data-testid="now-playing-announcer"
			className="sr-only"
			aria-live="polite"
			aria-atomic="true"
		>
			{message}
		</div>
	);
}
