import { describe, expect, it } from "vite-plus/test";
import {
	type RadioStationForm,
	radioStationFormSchema,
	radioStationQuickEditSchema,
} from "./radio-station-form";

const form: RadioStationForm = {
	name: "  Radio One ",
	streamUrl: " https://radio.example/stream ",
	homepageUrl: "",
	faviconUrl: " https://radio.example/icon.png",
	country: "DE ",
	language: "",
	tags: "jazz, , late night ,",
	codec: "MP3",
	bitrate: " 128 ",
};

describe("radioStationFormSchema", () => {
	it("turns the form into the trimmed patch it saves", () => {
		expect(radioStationFormSchema.parse(form)).toEqual({
			name: "Radio One",
			streamUrl: "https://radio.example/stream",
			homepageUrl: "",
			faviconUrl: "https://radio.example/icon.png",
			country: "DE",
			language: "",
			tags: ["jazz", "late night"],
			codec: "MP3",
			bitrate: 128,
		});
		expect(radioStationFormSchema.parse({ ...form, bitrate: "" }).bitrate).toBe(
			0,
		);
	});

	it("requires a name and a stream URL", () => {
		expect(
			radioStationFormSchema.safeParse({ ...form, name: "  " }).success,
		).toBe(false);
		expect(
			radioStationFormSchema.safeParse({ ...form, streamUrl: "" }).success,
		).toBe(false);
	});

	it("refuses a bitrate that is no whole, positive number", () => {
		for (const bitrate of ["fast", "-128"]) {
			expect(
				radioStationFormSchema.safeParse({ ...form, bitrate }).success,
			).toBe(false);
		}
	});
});

describe("radioStationQuickEditSchema", () => {
	it("trims the name and stream URL and requires both", () => {
		expect(
			radioStationQuickEditSchema.parse({
				name: " Radio One ",
				streamUrl: " https://radio.example/stream",
			}),
		).toEqual({ name: "Radio One", streamUrl: "https://radio.example/stream" });
		expect(
			radioStationQuickEditSchema.safeParse({
				name: "Radio One",
				streamUrl: " ",
			}).success,
		).toBe(false);
	});
});
