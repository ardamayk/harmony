// Custom request headers for one Music Server. They are secrets, so they stay
// out of the core's plain connection file: the shell keeps them encrypted and
// hands them to the core once at startup.

import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { CoreCommandError } from "./core-process";

export const SERVER_HEADERS_FILE_NAME = "server-headers.bin";

/** The part of Electron's `safeStorage` this uses. */
export type HeaderCipher = {
	isEncryptionAvailable(): boolean;
	/** Linux only: "basic_text" means a key hardcoded in Chromium. */
	getSelectedStorageBackend?(): string;
	encryptString(plainText: string): Buffer;
	decryptString(encrypted: Buffer): string;
};

export type SavedServerHeaders = {
	origin: string;
	headers: Record<string, string>;
};

export class ServerHeadersStore {
	constructor(
		private readonly path: string,
		private readonly cipher: HeaderCipher,
	) {}

	assertAvailable(): void {
		const backend =
			process.platform === "linux"
				? this.cipher.getSelectedStorageBackend?.()
				: undefined;
		if (!this.cipher.isEncryptionAvailable() || backend === "basic_text") {
			throw new CoreCommandError({
				code: "storage",
				message:
					"This device has no keychain to keep custom headers in. Start a Secret Service or KWallet, then try again.",
			});
		}
	}

	/** The saved headers; unreadable ones count as none, so the app still opens. */
	load(): SavedServerHeaders | null {
		try {
			const encrypted = readFileSync(this.path);
			return JSON.parse(
				this.cipher.decryptString(encrypted),
			) as SavedServerHeaders;
		} catch {
			return null;
		}
	}

	/**
	 * Records what the core just saved: new headers replace the old ones, and
	 * without any, headers that belong to another server are removed.
	 */
	sync(origin: string, headers: Record<string, string> | undefined): void {
		if (headers) {
			const plain = JSON.stringify({ origin, headers });
			writeFileSync(this.path, this.cipher.encryptString(plain), {
				mode: 0o600,
			});
		} else if (this.load()?.origin !== origin) {
			rmSync(this.path, { force: true });
		}
	}
}
