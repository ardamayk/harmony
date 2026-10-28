import type {
	ExternalTrack,
	RadioSearchResult,
	RadioStation,
	Track,
} from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import type { PlaybackEngine } from "../PlaybackEngine";

const track: Track = {
	id: "track-1",
	title: "Track 1",
	artistName: "Artist",
	artists: [],
	albumId: "album-1",
	discNo: 1,
	durationMs: 120000,
	format: "flac",
	genres: [],
};

export const trackSource = {
	type: "track" as const,
	track,
	playbackUrl: "/stream/track-1",
};

const radioStation: RadioStation = {
	id: "station-1",
	name: "Station 1",
	streamUrl: "https://example.com/live",
	tags: [],
	source: "manual",
	isFavorite: false,
	position: 0,
};

const catalogResult: RadioSearchResult = {
	stationUuid: "catalog-1",
	name: "Catalog 1",
	streamUrl: "https://example.com/preview",
	tags: [],
};

const externalTrack: ExternalTrack = {
	id: "external-1",
	title: "External 1",
	artistName: "Uploader",
	durationMs: 90000,
	webpageUrl: "https://example.com/track",
	extractor: "Soundcloud",
};

export type PlaybackEngineContractDriver = {
	engine: PlaybackEngine;
	finish(): void;
};

export function playbackEngineContract(
	name: string,
	createDriver: () => PlaybackEngineContractDriver,
) {
	describe(`${name} PlaybackEngine contract`, () => {
		it("starts idle and plays a Track", async () => {
			const { engine } = createDriver();

			expect(engine.getState()).toMatchObject({
				source: null,
				status: "idle",
				currentTime: 0,
				volume: 0.8,
				error: null,
			});

			await engine.play(trackSource);

			expect(engine.getState()).toMatchObject({
				source: trackSource,
				status: "playing",
				duration: 120,
				error: null,
			});
			engine.destroy();
		});

		it("plays Radio Station and Catalog Preview sources", async () => {
			const { engine } = createDriver();
			const radioSource = {
				type: "radio-station" as const,
				station: radioStation,
				playbackUrl: "/radio/station-1",
				sourceUrl: radioStation.streamUrl,
			};
			const catalogSource = {
				type: "catalog-preview" as const,
				result: catalogResult,
				playbackUrl: "/radio/preview/catalog-1",
				sourceUrl: catalogResult.streamUrl,
			};

			await engine.play(radioSource);
			expect(engine.getState()).toMatchObject({
				source: radioSource,
				status: "playing",
				duration: 0,
			});
			await engine.play(catalogSource);
			expect(engine.getState()).toMatchObject({
				source: catalogSource,
				status: "playing",
				duration: 0,
			});
			engine.destroy();
		});

		it("plays an External Track with its duration and seeks in it", async () => {
			const { engine } = createDriver();
			const externalSource = {
				type: "external-track" as const,
				externalTrack,
				playbackUrl: "/api/v1/external/tracks/external-1/stream",
				sourceUrl: "/api/v1/external/tracks/external-1/stream",
			};

			await engine.play(externalSource);
			engine.seek(30);

			expect(engine.getState()).toMatchObject({
				source: externalSource,
				status: "playing",
				currentTime: 30,
				duration: 90,
				error: null,
			});
			engine.destroy();
		});

		it("publishes pause, seek, volume, shuffle, and repeat state", async () => {
			const { engine } = createDriver();
			const observedStatuses: string[] = [];
			const unsubscribe = engine.subscribe((state) => {
				observedStatuses.push(state.status);
			});

			await engine.play(trackSource);
			engine.seek(25);
			engine.setVolume(2);
			engine.toggleShuffle();
			engine.cycleRepeatMode();
			engine.pause();

			expect(engine.getState()).toMatchObject({
				status: "paused",
				currentTime: 25,
				volume: 1,
				shuffleEnabled: true,
				repeatMode: "once",
			});
			expect(observedStatuses).toContain("playing");
			expect(observedStatuses.at(-1)).toBe("paused");
			unsubscribe();
			engine.destroy();
		});

		it("reports ended and applies repeat modes", async () => {
			const { engine, finish } = createDriver();
			await engine.play(trackSource);

			finish();
			expect(engine.getState().status).toBe("ended");

			await engine.play(trackSource);
			engine.cycleRepeatMode();
			finish();
			expect(engine.getState()).toMatchObject({
				status: "playing",
				currentTime: 0,
				repeatMode: "off",
			});

			engine.cycleRepeatMode();
			engine.cycleRepeatMode();
			finish();
			expect(engine.getState()).toMatchObject({
				status: "playing",
				repeatMode: "loop",
			});
			engine.destroy();
		});
	});
}
