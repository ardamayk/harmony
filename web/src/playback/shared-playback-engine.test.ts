import type { PlaybackSessionState, PlaybackSource } from "@repo/ui";
import { DEFAULT_PLAYBACK_SESSION_STATE } from "@repo/ui";
import { render, screen } from "@testing-library/react";
import { createElement, StrictMode, useEffect, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { DesktopPlaybackEngine } from "./DesktopPlaybackEngine";
import {
	disposeSharedPlaybackEngine,
	getSharedPlaybackEngine,
} from "./shared-playback-engine";

const source: PlaybackSource = {
	type: "track",
	track: {
		id: "track-1",
		title: "Track 1",
		artistName: "Artist",
		artists: [],
		albumId: "album-1",
		discNo: 1,
		durationMs: 120_000,
		format: "flac",
		genres: [],
	},
	playbackUrl: `http://127.0.0.1:43129/${"a".repeat(64)}/api/v1/tracks/track-1/stream`,
};

afterEach(() => disposeSharedPlaybackEngine());

describe("shared playback engine", () => {
	it("hydrates one native session across development StrictMode render replay", async () => {
		const restored: PlaybackSessionState = {
			...DEFAULT_PLAYBACK_SESSION_STATE,
			source,
			outputMode: "system",
			status: "paused",
		};
		const bridge = {
			listen: vi.fn(async () => () => undefined),
			rendererReady: vi.fn(async () => restored),
		};
		const createEngine = vi.fn(
			() => new DesktopPlaybackEngine(bridge as never),
		);

		function Probe() {
			const engine = getSharedPlaybackEngine(createEngine);
			const [state, setState] = useState(engine.getState());
			useEffect(() => {
				setState(engine.getState());
				return engine.subscribe(setState);
			}, [engine]);
			return createElement(
				"span",
				null,
				`${state.source?.type === "track" ? state.source.track.title : "Nothing playing"} / ${state.outputMode ?? "no mode"}`,
			);
		}

		render(createElement(StrictMode, null, createElement(Probe)));

		expect(await screen.findByText("Track 1 / system")).toBeTruthy();
		expect(createEngine).toHaveBeenCalledOnce();
		expect(bridge.rendererReady).toHaveBeenCalledOnce();
	});
});
