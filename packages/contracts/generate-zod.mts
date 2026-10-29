// Generates the api-client's zod response schemas from the OpenAPI contract
// with @hey-api/openapi-ts (zod plugin only: the client keeps its own
// request helper and openapi-typescript types). Run by
// scripts/generate-openapi.sh: `node generate-zod.mts <openapi.yaml> <outdir>`.
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createClient } from "@hey-api/openapi-ts";

const [input, output] = process.argv.slice(2);
if (!input || !output) {
	console.error("Usage: node generate-zod.mts <openapi.yaml> <output-dir>");
	process.exit(2);
}

await createClient({
	input,
	// Writes zod.gen.ts next to schema.ts; `clean` would delete schema.ts.
	output: { path: output, clean: false, postProcess: [] },
	logs: { level: "silent" },
	plugins: [
		{
			name: "zod",
			// RFC 3339 times carry an offset (`+00:00`) as well as `Z`.
			dates: { offset: true },
			// Only responses are validated (ADR 0034).
			requests: false,
		},
	],
});

// `format: uuid` in the contract means any 8-4-4-4-12 hex id. zod's
// z.uuid() only accepts RFC 9562 versions 1-8, so it refuses the Music
// Server's single user, 00000000-0000-0000-0000-000000000001; z.guid()
// checks the shape alone.
const zodFile = join(output, "zod.gen.ts");
const generated = await readFile(zodFile, "utf8");
await writeFile(zodFile, generated.replaceAll("z.uuid()", "z.guid()"));
