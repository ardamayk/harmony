---
status: accepted
---

# Scalar replaces Swagger UI, served by the Music Server

`/api/docs` showed Swagger UI loaded from unpkg as `swagger-ui-dist@5`, an unpinned major version. Every visit fetched code from a CDN, and the version could change under us without a commit.

The page is now Scalar's API reference (`@scalar/api-reference`), self-hosted:

- The version is pinned exactly in `packages/docs/package.json` and the lockfile, and was published at least two weeks before it was adopted. Upgrades are ordinary dependency changes.
- `scripts/sync-static.sh api-reference` copies the package's prebuilt standalone browser bundle (`dist/browser/standalone.js`) into `server/static/api-reference`. The Music Server embeds it like the Web Client and product docs and serves it at `/api/docs/scalar/standalone.js`. `mise run server:build`, `mise run dev` and the Docker build all run the copy.
- The page (`server/api-reference.html`) and its configuration (`server/api-reference.js`) are compiled into the server. Scalar reads `/api/openapi.yaml`, and "Try it" requests go to the origin that served the page.
- No request leaves the page's origin. Scalar's default fonts (fonts.scalar.com), telemetry, AI agent, MCP links and developer tools are switched off. The response's Content-Security-Policy allows only `'self'` for scripts, connections, fonts and images, so anything missed is blocked rather than sent. Styles also allow `'unsafe-inline'`, because Scalar sets `style` attributes and injects `<style>` elements at runtime.
- The theme is Scalar's built-in one. It follows the system's light or dark setting; the page adds no design tokens of its own.

## Considered Options

- Keep Swagger UI but self-host and pin `swagger-ui-dist`: rejected. It would remove the CDN dependency, but the project chose Scalar's reference for the page.
- Load Scalar from a CDN with a pinned version: rejected. The docs page would still depend on a third-party host and send it every visit.
- Bundle Scalar into the product docs build: rejected. The prebuilt standalone bundle needs no build step of ours, and copying it keeps the docs build unchanged.

## Consequences

- The Music Server binary grows by the bundle's size (about 4.5 MB).
- `mise run server:test` copies the bundle before testing, so the server's CI job runs `pnpm install` too.
- Scalar's "Generate SDK" dialog still appears on localhost and links to Scalar's dashboard if clicked; it sends nothing on its own, and the policy blocks its background requests.
