import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vite-plus/test";
import {
	createRendererHandler,
	rendererFile,
	resolveRendererPath,
} from "./renderer-protocol";
import { CONTENT_SECURITY_POLICY } from "./security";

const root = mkdtempSync(join(tmpdir(), "earthly-renderer-"));
mkdirSync(join(root, "assets"));
writeFileSync(join(root, "index.html"), "<!doctype html>");
writeFileSync(join(root, "assets/app.js"), "export {}");

afterAll(() => rmSync(root, { recursive: true, force: true }));

describe("resolveRendererPath", () => {
	it("maps app://earthly paths into the renderer root", () => {
		expect(resolveRendererPath(root, "app://earthly/assets/app.js")).toBe(
			join(root, "assets/app.js"),
		);
	});

	it("refuses other hosts and paths that leave the root", () => {
		expect(resolveRendererPath(root, "app://other/index.html")).toBeNull();
		// The URL parser already folds dot segments back into the root.
		expect(resolveRendererPath(root, "app://earthly/%2e%2e/secret")).toBe(
			join(root, "secret"),
		);
		expect(
			resolveRendererPath(root, "app://earthly/..%2f..%2fetc/passwd"),
		).toBeNull();
	});
});

describe("rendererFile", () => {
	it("serves assets as they are and routes through the SPA entry", async () => {
		expect(await rendererFile(root, "app://earthly/assets/app.js")).toBe(
			join(root, "assets/app.js"),
		);
		expect(await rendererFile(root, "app://earthly/")).toBe(
			join(root, "index.html"),
		);
		expect(await rendererFile(root, "app://earthly/mini")).toBe(
			join(root, "index.html"),
		);
		expect(await rendererFile(root, "app://earthly/library/albums/abc")).toBe(
			join(root, "index.html"),
		);
	});

	it("answers a missing asset with nothing rather than the SPA", async () => {
		expect(
			await rendererFile(root, "app://earthly/assets/missing.js"),
		).toBeNull();
	});
});

describe("createRendererHandler", () => {
	it("adds the CSP to every response", async () => {
		const handler = createRendererHandler(root, async () => new Response("ok"));
		const response = await handler(new Request("app://earthly/mini"));

		expect(response.status).toBe(200);
		expect(response.headers.get("Content-Security-Policy")).toBe(
			CONTENT_SECURITY_POLICY,
		);
	});

	it("refuses writes and unknown files", async () => {
		const handler = createRendererHandler(root, async () => new Response("ok"));

		expect(
			(await handler(new Request("app://earthly/", { method: "POST" }))).status,
		).toBe(405);
		expect((await handler(new Request("app://earthly/x.png"))).status).toBe(
			404,
		);
	});
});
