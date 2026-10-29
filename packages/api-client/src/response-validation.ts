// Development-time check that Music Server responses match the contract
// (ADR 0034). The zod schemas are generated from packages/contracts by
// `mise run generate` and loaded lazily, so a production build that leaves
// validation off never downloads or runs them.

import type { operations } from "./generated/schema";

/** An `operationId` from the OpenAPI contract. */
export type ApiOperation = keyof operations;

/** At most this many issue paths are named in an error message. */
const MAX_REPORTED_PATHS = 10;

/**
 * A response did not match its operation's schema. The message names the
 * operation and the paths that failed, never the response's values.
 */
export class ApiResponseValidationError extends Error {
	constructor(
		public readonly operation: ApiOperation,
		public readonly status: number,
		/** Dotted paths into the response body, `(root)` for the body itself. */
		public readonly issuePaths: string[],
	) {
		const shown = issuePaths.slice(0, MAX_REPORTED_PATHS);
		const more = issuePaths.length - shown.length;
		super(
			`Response of ${operation} (HTTP ${status}) does not match the API contract at ${shown.join(", ")}${more > 0 ? ` and ${more} more` : ""}.`,
		);
		this.name = "ApiResponseValidationError";
	}
}

type ResponseSchema = {
	safeParse(
		value: unknown,
	):
		| { success: true }
		| { success: false; error: { issues: { path: PropertyKey[] }[] } };
};

let schemas: Promise<Record<string, ResponseSchema | undefined>> | undefined;

function responseSchemas() {
	schemas ??= import("./generated/zod.gen") as Promise<
		Record<string, ResponseSchema | undefined>
	>;
	return schemas;
}

/** The generated name of an operation's success response schema. */
function schemaName(operation: ApiOperation): string {
	return `z${operation.charAt(0).toUpperCase()}${operation.slice(1)}Response`;
}

/**
 * Throws ApiResponseValidationError when `body` does not match the
 * operation's success response. The body itself is left as it is.
 */
export async function validateResponse(
	operation: ApiOperation,
	status: number,
	body: unknown,
): Promise<void> {
	const schema = (await responseSchemas())[schemaName(operation)];
	if (schema === undefined) {
		throw new Error(
			`The API contract has no response schema for ${operation}.`,
		);
	}
	const result = schema.safeParse(body);
	if (result.success) return;
	const paths = result.error.issues.map((issue) =>
		issue.path.length === 0 ? "(root)" : issue.path.map(String).join("."),
	);
	throw new ApiResponseValidationError(operation, status, [...new Set(paths)]);
}

/**
 * Whether this runs in development or a test: `process.env.NODE_ENV` is set
 * and is not "production". Browsers have no `process`; the Web Client passes
 * its build mode explicitly.
 */
export function isDevelopmentBuild(): boolean {
	const nodeEnv = (
		globalThis as { process?: { env?: Record<string, string | undefined> } }
	).process?.env?.NODE_ENV;
	return nodeEnv !== undefined && nodeEnv !== "production";
}
