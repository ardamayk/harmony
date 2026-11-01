import { describe, expect, it } from "vite-plus/test";
import { parseInvoke } from "./schemas";

const claim = {
	type: "claim",
	deviceId: "018f6a52-1d3f-7cc1-9a4f-6f9db4a5d0e1",
	deviceName: "Harmony on music-desk",
	queueItemId: null,
	trackId: "track-1",
	positionMs: 81_250,
	playing: true,
};

describe("shared playback command schema", () => {
	it("accepts the command shapes the contract defines", () => {
		for (const command of [
			claim,
			{
				type: "report",
				deviceId: "device-1",
				basedOnRevision: 12,
				queueItemId: "item-1",
				trackId: null,
				positionMs: 0,
				playing: false,
			},
			{ type: "pause" },
			{ type: "seek", positionMs: 30_000 },
			{ type: "skip", direction: "previous" },
		]) {
			const parsed = parseInvoke("send_shared_playback_command", {
				command,
			});
			expect(parsed).toMatchObject({ ok: true });
		}
	});

	it("refuses commands that break the contract", () => {
		for (const command of [
			{ ...claim, deviceId: "" },
			{ ...claim, positionMs: -1 },
			{ ...claim, playing: "yes" },
			{ ...claim, extra: true },
			{ type: "seek", positionMs: -5 },
			{ type: "skip", direction: "up" },
			{ type: "rewind" },
		]) {
			const parsed = parseInvoke("send_shared_playback_command", {
				command,
			});
			expect(parsed.ok).toBe(false);
		}
	});

	it("takes a Room's suspension as a boolean", () => {
		expect(
			parseInvoke("set_shared_playback_suspended", { suspended: true }),
		).toMatchObject({ ok: true });
		expect(
			parseInvoke("set_shared_playback_suspended", { suspended: "yes" }).ok,
		).toBe(false);
	});
});
