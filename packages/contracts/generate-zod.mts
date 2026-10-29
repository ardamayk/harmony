// Generates the api-client's zod response schemas from the OpenAPI contract
// with @hey-api/openapi-ts (zod plugin only: the client keeps its own
// request helper and openapi-typescript types). Run by
// scripts/generate-openapi.sh: `node generate-zod.mts <openapi.yaml> <outdir>`.
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
