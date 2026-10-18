import type { PlaybackSessionState, PlaybackSource } from "@repo/ui";
import { DEFAULT_PLAYBACK_SESSION_STATE } from "@repo/ui";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { installDesktopBridge } from "#/desktop/testing";
import { createPlaybackEngine } from "./create-playback-engine";
import { DesktopPlaybackEngine } from "./DesktopPlaybackEngine";

describe("createPlaybackEngine", () => {
	afterEach(() => {
		Reflect.deleteProperty(window, "earthlyDesktop");
		vi.unstubAllGlobals();
	});

	it("uses native playback in Desktop without creating HTML audio", () => {
		installDesktopBridge();
		const audioConstructor = vi.fn();
		vi.stubGlobal("Audio", audioConstructor);

		const engine = createPlaybackEngine();

		expect(engine).toBeInstanceOf(DesktopPlaybackEngine);
		expect(audioConstructor).not.toHaveBeenCalled();
		engine.destroy();
	});

	it("smoke plays saved Radio and Catalog Preview sources through Desktop commands", async () => {
		const playedSources: PlaybackSource[] = [];
		let state: PlaybackSessionState = { ...DEFAULT_PLAYBACK_SESSION_STATE };
		installDesktopBridge((command, args) => {
			if (command === "desktop_playback_renderer_ready") return state;
			if (command === "desktop_playback_play") {
				if (!args || !("source" in args)) {
					throw new Error("Desktop playback command source is missing");
				}
				const source = args.source as PlaybackSource;
				playedSources.push(source);
				state = { ...state, source, status: "playing" };
				return state;
			}
			return state;
		});
		const engine = createPlaybackEngine();
		const radioSource: PlaybackSource = {
			type: "radio-station",
			station: {
				id: "station-1",
				name: "Station 1",
				streamUrl: "https://radio.example/live.mp3",
				tags: [],
				source: "manual",
				isFavorite: false,
				position: 0,
			},
			playbackUrl:
				"http://127.0.0.1:43129/token/api/v1/radio/stations/station-1/stream",
			sourceUrl: "https://radio.example/live.mp3",
		};
		const catalogSource: PlaybackSource = {
			type: "catalog-preview",
			result: {
				stationUuid: "catalog-1",
				name: "Catalog 1",
				streamUrl: "https://catalog.example/live.m3u8",
				tags: [],
			},
			playbackUrl:
				"http://127.0.0.1:43129/token/api/v1/radio/catalog/catalog-1/stream",
			sourceUrl: "https://catalog.example/live.m3u8",
		};

		await engine.play(radioSource);
		await engine.play(catalogSource);

		expect(playedSources).toHaveLength(2);
		expect(playedSources).toContainEqual(radioSource);
		expect(playedSources).toContainEqual(catalogSource);
		expect(engine.getState().source).toEqual(catalogSource);
		engine.destroy();
	});
});
