---
status: accepted
---

# API responses are validated against the contract in development only

The api-client trusted every Music Server response as the type the contract promises. A server change that drifts from `packages/contracts/openapi.yaml` then surfaced far from its cause, as an `undefined` deep in a view.

So `mise run generate` also writes zod schemas for every operation's response (`packages/api-client/src/generated/zod.gen.ts`, by `@hey-api/openapi-ts`'s zod plugin), committed and drift-checked like the TypeScript types (ADR 0013). With `validateResponses` on, the request helper parses each JSON response with its operation's schema and throws `ApiResponseValidationError`, which names the `operationId` and the failing paths but never the response's values. The response itself is returned as received, not the parsed copy.

Validation is on in development and tests and off in production builds: the Web Client passes `import.meta.env.DEV`, and without the option the client follows `process.env.NODE_ENV`, so the package needs no Vite globals. The schemas load through a dynamic import, so production never downloads them.

## Considered Options

- Validate in production too: rejected. Library lists hold thousands of items, and a contract mismatch would break pages that render fine today.
- Hand-written schemas: rejected; they drift from the contract, which is the source of truth.
- `openapi-zod-client` or Orval: rejected. The former targets zod 3 and is barely maintained; Orval brings its own client and much more than the schemas. `@hey-api/openapi-ts` is maintained, emits zod 4 and can emit the schemas alone.

## Consequences

- Test fixtures must match the contract (for example UUID ids) wherever a test builds a real client.
- A response the contract does not describe fails loudly in development; fix the server or the contract, not the check.
