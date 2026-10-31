import { ApiError } from "@repo/api-client";
import { describe, expect, it } from "vite-plus/test";
import { readDesktopUpload } from "./desktop-upload";

describe("readDesktopUpload", () => {
	it("returns a successful answer's preview", async () => {
		const response = new Response(JSON.stringify({ jobId: "job-1" }));
		await expect(readDesktopUpload(response)).resolves.toEqual({
			jobId: "job-1",
		});
	});

	it("keeps the Music Server's own error", async () => {
		const response = new Response(
			JSON.stringify({ error: "x", code: "too_big", message: "Too big" }),
			{ status: 422 },
		);
		const error = await readDesktopUpload(response).catch((cause) => cause);
		expect(error).toBeInstanceOf(ApiError);
		expect(error.status).toBe(422);
		expect(error.message).toBe("Too big");
	});

	it("names the status of a proxy's HTML page", async () => {
		const response = new Response("<html>\n<head><title>413</title>", {
			status: 413,
		});
		const error = await readDesktopUpload(response).catch((cause) => cause);
		expect(error).toBeInstanceOf(ApiError);
		expect(error.status).toBe(413);
		expect(error.message).toBe("HTTP 413");
	});
});
