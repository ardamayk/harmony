// The Rust core as a child process speaking line-delimited JSON-RPC (see
// desktop/core/src/rpc.rs): requests go to its stdin, responses and events
// come back on its stdout. Its stderr is passed through for diagnostics.

import { type ChildProcess, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { createInterface } from "node:readline";

type Pending = {
	resolve: (value: unknown) => void;
	reject: (error: unknown) => void;
};

type CoreMessage =
	| { id: number; result?: unknown; error?: unknown }
	| { event: string; payload: unknown };

/** Lines of stderr kept to explain a core that fails to start. */
const STDERR_TAIL_LINES = 20;

export type CoreProcessOptions = {
	executable: string;
	args: string[];
	env?: NodeJS.ProcessEnv;
};

/**
 * Error raised for a request the core answered with an error value; `value`
 * is the command's own error shape, passed to the renderer unchanged.
 */
export class CoreCommandError extends Error {
	constructor(readonly value: unknown) {
		super(
			typeof value === "string"
				? value
				: ((value as { message?: string } | null)?.message ??
						"Desktop core command failed."),
		);
	}
}

export interface CoreProcessEvents {
	event: [name: string, payload: unknown];
	exit: [code: number | null, signal: NodeJS.Signals | null];
}

export class CoreProcess extends EventEmitter<CoreProcessEvents> {
	private readonly child: ChildProcess;
	private readonly pending = new Map<number, Pending>();
	private readonly stderrTail: string[] = [];
	private nextId = 1;
	private exitPromise: Promise<void>;
	exited = false;

	constructor({ executable, args, env }: CoreProcessOptions) {
		super();
		this.child = spawn(executable, args, {
			env,
			stdio: ["pipe", "pipe", "pipe"],
			// Its own process group: Ctrl+C in a terminal reaches only the
			// shell, which then stops the core through stdin so it can save
			// the Playback Session. A crashed shell closes stdin all the same.
			detached: true,
		});
		this.exitPromise = new Promise((resolve) => {
			const finish = (code: number | null, signal: NodeJS.Signals | null) => {
				if (this.exited) return;
				this.exited = true;
				const reason = new CoreCommandError(
					"The Desktop core stopped before answering.",
				);
				for (const { reject } of this.pending.values()) reject(reason);
				this.pending.clear();
				this.emit("exit", code, signal);
				resolve();
			};
			this.child.once("exit", finish);
			this.child.on("error", (error) => {
				this.stderrTail.push(error.message);
				// A core that never started (missing or not executable) emits
				// no "exit"; report it the same way so the shell does not hang.
				if (this.child.pid === undefined) finish(null, null);
			});
		});
		const stdout = this.child.stdout;
		const stderr = this.child.stderr;
		if (!stdout || !stderr) throw new Error("Desktop core pipes are missing.");
		createInterface({ input: stdout }).on("line", (line) =>
			this.handleLine(line),
		);
		createInterface({ input: stderr }).on("line", (line) => {
			process.stderr.write(`[core] ${line}\n`);
			this.stderrTail.push(line);
			if (this.stderrTail.length > STDERR_TAIL_LINES) this.stderrTail.shift();
		});
	}

	/** The last lines the core wrote to stderr, for an error dialog. */
	get diagnostics(): string {
		return this.stderrTail.join("\n");
	}

	request(method: string, params?: Record<string, unknown>): Promise<unknown> {
		if (this.exited || !this.child.stdin?.writable) {
			return Promise.reject(
				new CoreCommandError("The Desktop core is not running."),
			);
		}
		const id = this.nextId++;
		return new Promise((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
			this.child.stdin?.write(
				`${JSON.stringify({ id, method, params: params ?? null })}\n`,
			);
		});
	}

	/**
	 * Closes stdin, which makes the core save the Playback Session and exit,
	 * and waits for it; a core that hangs is killed after `timeoutMs`.
	 */
	async stop(timeoutMs = 5000): Promise<void> {
		if (!this.exited) this.child.stdin?.end();
		let timer: NodeJS.Timeout | undefined;
		const timedOut = new Promise<"timeout">((resolve) => {
			timer = setTimeout(() => resolve("timeout"), timeoutMs);
		});
		const result = await Promise.race([this.exitPromise, timedOut]);
		clearTimeout(timer);
		if (result === "timeout" && !this.exited) {
			process.stderr.write("[core] did not exit in time; killing it\n");
			this.child.kill("SIGKILL");
			await this.exitPromise;
		}
	}

	private handleLine(line: string) {
		if (!line.trim()) return;
		let message: CoreMessage;
		try {
			message = JSON.parse(line) as CoreMessage;
		} catch {
			process.stderr.write(`[core] unparseable output: ${line}\n`);
			return;
		}
		if ("event" in message) {
			this.emit("event", message.event, message.payload);
			return;
		}
		const pending = this.pending.get(message.id);
		if (!pending) return;
		this.pending.delete(message.id);
		if ("error" in message) pending.reject(new CoreCommandError(message.error));
		else pending.resolve(message.result ?? null);
	}
}
