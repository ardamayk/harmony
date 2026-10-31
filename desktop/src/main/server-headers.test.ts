import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vite-plus/test";
import { type HeaderCipher, ServerHeadersStore } from "./server-headers";

const ORIGIN = "https://music.example.com";
const HEADERS = { "X-Proxy-Key": "secret" };

const cipher = (overrides: Partial<HeaderCipher> = {}): HeaderCipher => ({
	isEncryptionAvailable: () => true,
	getSelectedStorageBackend: () => "gnome_libsecret",
	encryptString: (plain) => Buffer.from(plain).reverse(),
	decryptString: (encrypted) => Buffer.from(encrypted).reverse().toString(),
	...overrides,
});

function store(overrides?: Partial<HeaderCipher>) {
	const path = join(mkdtempSync(join(tmpdir(), "headers-")), "headers.bin");
	return { path, headers: new ServerHeadersStore(path, cipher(overrides)) };
}

describe("ServerHeadersStore", () => {
	it("writes only the encrypted form and reads it back", () => {
		const { path, headers } = store();
		headers.sync(ORIGIN, HEADERS);

		expect(readFileSync(path).toString()).not.toContain("secret");
		expect(headers.load()).toEqual({ origin: ORIGIN, headers: HEADERS });
	});

	it("has nothing to load before anything is saved", () => {
		expect(store().headers.load()).toBeNull();
	});

	it("treats a file it cannot decrypt as nothing saved", () => {
		const { path, headers } = store();
		headers.sync(ORIGIN, HEADERS);
		const broken = new ServerHeadersStore(
			path,
			cipher({
				decryptString: () => {
					throw new Error("the keychain changed");
				},
			}),
		);
		expect(broken.load()).toBeNull();
	});

	it("keeps headers when the same server is saved without new ones", () => {
		const { headers } = store();
		headers.sync(ORIGIN, HEADERS);
		headers.sync(ORIGIN, undefined);
		expect(headers.load()?.headers).toEqual(HEADERS);
	});

	it("removes another server's headers", () => {
		const { path, headers } = store();
		headers.sync(ORIGIN, HEADERS);
		headers.sync("http://127.0.0.1:8090", undefined);
		expect(existsSync(path)).toBe(false);
	});

	it("refuses to save without real encryption", () => {
		expect(() =>
			store({ isEncryptionAvailable: () => false }).headers.assertAvailable(),
		).toThrow("keychain");
		if (process.platform === "linux") {
			expect(() =>
				store({
					getSelectedStorageBackend: () => "basic_text",
				}).headers.assertAvailable(),
			).toThrow("keychain");
		}
		expect(() => store().headers.assertAvailable()).not.toThrow();
	});
});
