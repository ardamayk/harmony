// Renderer trust boundary for the Desktop Client (issue #55), formerly pinned
// in the Tauri capability and config tests.

import { CORE_COMMANDS, SHELL_COMMANDS } from "@repo/desktop-bridge";
import { describe, expect, it } from "vite-plus/test";
import {
	CONTENT_SECURITY_POLICY,
	isTrustedRendererUrl,
	MEDIA_PROXY_ORIGIN,
	rendererWebPreferences,
	validateExternalUrl,
} from "./security";

function directive(name: string): string[] {
	const found = CONTENT_SECURITY_POLICY.split(";")
		.map((part) => part.trim())
		.find((part) => part.startsWith(`${name} `));
	if (!found) throw new Error(`missing ${name}`);
	return found.split(/\s+/).slice(1);
}

describe("renderer content security policy", () => {
	it("lets renderer fetches reach only the app and the private media proxy", () => {
		expect(directive("connect-src")).toEqual(["'self'", MEDIA_PROXY_ORIGIN]);
		expect(directive("media-src")).toEqual([MEDIA_PROXY_ORIGIN]);
		expect(directive("img-src")).toEqual([
			"'self'",
			MEDIA_PROXY_ORIGIN,
			"data:",
		]);
	});

	it("allows no wildcard or scheme-wide source", () => {
		for (const part of CONTENT_SECURITY_POLICY.split(";")) {
			for (const source of part.trim().split(/\s+/).slice(1)) {
				expect(["*", "http:", "https:", "ws:", "wss:"]).not.toContain(source);
				expect(source).not.toContain("*");
			}
		}
		expect(CONTENT_SECURITY_POLICY).not.toContain("'unsafe-eval'");
	});
});

describe("renderer web preferences", () => {
	it("isolate the page from Node.js and Electron", () => {
		const preferences = rendererWebPreferences("/app/preload.cjs");
		expect(preferences).toMatchObject({
			preload: "/app/preload.cjs",
			contextIsolation: true,
			sandbox: true,
			nodeIntegration: false,
			nodeIntegrationInWorker: false,
			webSecurity: true,
			allowRunningInsecureContent: false,
		});
	});
});

describe("trusted renderer URLs", () => {
	it("accept the bundled renderer and, in development, the dev server", () => {
		expect(isTrustedRendererUrl("app://earthly/library/albums", null)).toBe(
			true,
		);
		expect(
			isTrustedRendererUrl(
				"http://localhost:3000/mini",
				"http://localhost:3000",
			),
		).toBe(true);
	});

	it("refuse everything else", () => {
		expect(isTrustedRendererUrl("app://other/", null)).toBe(false);
		expect(isTrustedRendererUrl("http://localhost:3000/", null)).toBe(false);
		expect(
			isTrustedRendererUrl("http://localhost:4000/", "http://localhost:3000"),
		).toBe(false);
		expect(isTrustedRendererUrl("https://attacker.example/", null)).toBe(false);
		expect(isTrustedRendererUrl("not a url", null)).toBe(false);
	});
});

describe("external links", () => {
	it("accept only web URLs", () => {
		expect(validateExternalUrl("https://open.spotify.com/search/x")).toBe(
			"https://open.spotify.com/search/x",
		);
		expect(validateExternalUrl("http://example.com")).toBe(
			"http://example.com/",
		);
		expect(() => validateExternalUrl("file:///etc/passwd")).toThrow(
			/not allowed/,
		);
		expect(() => validateExternalUrl("javascript:alert(1)")).toThrow(
			/not allowed/,
		);
		expect(() => validateExternalUrl("not a url")).toThrow(/not a valid URL/);
		expect(() => validateExternalUrl(42)).toThrow(/not a valid URL/);
	});
});

describe("command allowlist", () => {
	it("never exposes shell-only core methods to the renderer", () => {
		const allowed: readonly string[] = [...CORE_COMMANDS, ...SHELL_COMMANDS];
		expect(allowed).not.toContain("desktop_register_import_paths");
		expect(allowed).not.toContain("shell_action");
		expect(new Set(allowed).size).toBe(allowed.length);
	});
});
