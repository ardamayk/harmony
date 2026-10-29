import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { ApiResponseValidationError, createApiClient } from "./index";
import { isDevelopmentBuild } from "./response-validation";

const ARTIST_ID = "00000000-0000-4000-8000-000000000001";

const validArtists = {
	items: [{ id: ARTIST_ID, name: "Artist", albumCount: 2, trackCount: 9 }],
	total: 1,
	limit: 50,
	offset: 0,
};

// The count is text and the name a number; neither value may leak into the
// error message.
const invalidArtists = {
	...validArtists,
	items: [{ ...validArtists.items[0], name: 404, albumCount: "secret-count" }],
};

function clientAnswering(body: unknown, validateResponses?: boolean) {
	return createApiClient({
		baseUrl: "http://music.test",
		transport: vi.fn<typeof fetch>().mockResolvedValue(Response.json(body)),
		validateResponses,
	});
}

afterEach(() => {
	vi.unstubAllEnvs();
});

describe("response validation on (development and tests)", () => {
	it("passes a response that matches the contract through unchanged", async () => {
		const withExtraField = { ...validArtists, nextCursor: "kept" };

		await expect(
			clientAnswering(withExtraField, true).listArtists(),
		).resolves.toEqual(withExtraField);
	});

	it("accepts the Music Server's single user id, which is not an RFC 9562 UUID", async () => {
		const me = {
			id: "00000000-0000-0000-0000-000000000001",
			username: "admin",
		};

		await expect(clientAnswering(me, true).getMe()).resolves.toEqual(me);
	});

	it("rejects a mismatch, naming the operation and paths but no values", async () => {
		const error = await clientAnswering(invalidArtists, true)
			.listArtists()
			.catch((caught: unknown) => caught);

		expect(error).toBeInstanceOf(ApiResponseValidationError);
		expect(error).toMatchObject({
			name: "ApiResponseValidationError",
			operation: "listArtists",
			status: 200,
			issuePaths: ["items.0.name", "items.0.albumCount"],
		});
		const { message } = error as Error;
		expect(message).toContain("listArtists");
		expect(message).toContain("items.0.albumCount");
		expect(message).not.toContain("secret-count");
		expect(message).not.toContain("404");
	});

	it("checks the whole body shape, including a missing body", async () => {
		await expect(clientAnswering(null, true).getHealth()).rejects.toMatchObject(
			{ operation: "getHealth", issuePaths: ["(root)"] },
		);
	});

	it("is on by default when NODE_ENV names a development or test run", async () => {
		vi.stubEnv("NODE_ENV", "test");
		expect(isDevelopmentBuild()).toBe(true);
		await expect(
			clientAnswering(invalidArtists).listArtists(),
		).rejects.toBeInstanceOf(ApiResponseValidationError);
	});
});

describe("response validation off (production builds)", () => {
	it("returns a valid response as it is", async () => {
		await expect(
			clientAnswering(validArtists, false).listArtists(),
		).resolves.toEqual(validArtists);
	});

	it("returns a mismatching response without checking it", async () => {
		await expect(
			clientAnswering(invalidArtists, false).listArtists(),
		).resolves.toEqual(invalidArtists);
	});

	it("is off by default in a production build", async () => {
		vi.stubEnv("NODE_ENV", "production");
		expect(isDevelopmentBuild()).toBe(false);
		await expect(
			clientAnswering(invalidArtists).listArtists(),
		).resolves.toEqual(invalidArtists);
	});
});
