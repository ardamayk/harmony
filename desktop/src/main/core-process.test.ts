import { describe, expect, it } from "vite-plus/test";
import { CoreCommandError, CoreProcess } from "./core-process";

// A stand-in core speaking the same line protocol as desktop/core/src/rpc.rs.
const FAKE_CORE = `
const lines = require("node:readline").createInterface({ input: process.stdin });
const out = (message) => process.stdout.write(JSON.stringify(message) + "\\n");
out({ event: "shell:ready", payload: { mediaProxyUrl: "http://127.0.0.1:43129/x" } });
lines.on("line", (line) => {
	const { id, method, params } = JSON.parse(line);
	if (method === "echo") out({ id, result: params });
	else if (method === "fail") out({ id, error: { code: "invalid_origin" } });
	else if (method === "hang") {}
	else out({ id, error: "Unknown desktop command '" + method + "'." });
});
lines.on("close", () => {
	console.error("saving playback");
	out({ event: "shell:exit", payload: null });
	process.exit(0);
});
`;

function startFakeCore() {
	return new CoreProcess({
		executable: process.execPath,
		args: ["-e", FAKE_CORE],
	});
}

describe("CoreProcess", () => {
	it("matches responses to requests and surfaces events", async () => {
		const core = startFakeCore();
		const events: string[] = [];
		core.on("event", (name) => events.push(name));
		try {
			await expect(core.request("echo", { seconds: 4 })).resolves.toEqual({
				seconds: 4,
			});
			const failure: unknown = await core
				.request("fail")
				.catch((error: unknown) => error);
			expect(failure).toBeInstanceOf(CoreCommandError);
			expect((failure as CoreCommandError).value).toEqual({
				code: "invalid_origin",
			});
			expect(events).toContain("shell:ready");
		} finally {
			await core.stop();
		}
	});

	it("stops by closing stdin and rejects what is still pending", async () => {
		const core = startFakeCore();
		const events: string[] = [];
		core.on("event", (name) => events.push(name));
		const pending = core.request("hang");
		await core.request("echo");

		await core.stop();

		expect(core.exited).toBe(true);
		expect(events).toContain("shell:exit");
		expect(core.diagnostics).toContain("saving playback");
		await expect(pending).rejects.toBeInstanceOf(CoreCommandError);
		await expect(core.request("echo")).rejects.toBeInstanceOf(CoreCommandError);
	});

	it("kills a core that does not exit in time", async () => {
		const core = new CoreProcess({
			executable: process.execPath,
			args: ["-e", "process.stdin.resume(); setInterval(() => {}, 1000);"],
		});
		const exited = new Promise((resolve) =>
			core.once("exit", (_code, signal) => resolve(signal)),
		);

		await core.stop(100);

		await expect(exited).resolves.toBe("SIGKILL");
	});
});
