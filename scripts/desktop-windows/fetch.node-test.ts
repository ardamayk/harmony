import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";
import { download, sha256File } from "./fetch.ts";

const body = "earthly audio\n";
const bodySha256 = createHash("sha256").update(body).digest("hex");
const work = mkdtempSync(join(tmpdir(), "earthly-audio-fetch-test-"));
let server: Server;
let baseUrl = "";
let requests = 0;

before(async () => {
	// A local server keeps the test hermetic; /redirect mimics GitHub assets.
	server = createServer((request, response) => {
		requests++;
		if (request.url === "/redirect/file.txt") {
			response.writeHead(302, { location: "/file.txt" }).end();
			return;
		}
		response.writeHead(200).end(body);
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

after(() => {
	server.close();
	rmSync(work, { recursive: true, force: true });
});

test("sha256File hashes the file's bytes", () => {
	const path = join(work, "hash.txt");
	writeFileSync(path, body);
	assert.equal(sha256File(path), bodySha256);
});

test("download follows redirects and reuses the verified cache", async () => {
	const cache = join(work, "cache");
	requests = 0;
	const path = await download(
		`${baseUrl}/redirect/file.txt`,
		bodySha256,
		cache,
	);
	assert.equal(path, join(cache, "file.txt"));
	assert.equal(sha256File(path), bodySha256);
	assert.equal(requests, 2);

	const modified = statSync(path).mtimeMs;
	assert.equal(await download(`${baseUrl}/file.txt`, bodySha256, cache), path);
	assert.equal(requests, 2);
	assert.equal(statSync(path).mtimeMs, modified);
});

test("download replaces a cached file whose hash differs", async () => {
	const cache = join(work, "stale");
	const path = await download(`${baseUrl}/file.txt`, bodySha256, cache);
	writeFileSync(path, "tampered");
	requests = 0;
	await download(`${baseUrl}/file.txt`, bodySha256, cache);
	assert.equal(requests, 1);
	assert.equal(sha256File(path), bodySha256);
});

test("download throws on a wrong sha256 and caches nothing", async () => {
	const cache = join(work, "wrong");
	await assert.rejects(
		download(`${baseUrl}/file.txt`, "0".repeat(64), cache),
		/sha256/,
	);
	assert.throws(() => statSync(join(cache, "file.txt")), { code: "ENOENT" });
});
