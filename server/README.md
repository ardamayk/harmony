# Earthly Audio Music Server

The Music Server, written in Rust (crate `earthly-music-server`; axum on tokio, sqlx over SQLite). It serves the HTTP contract in `packages/contracts/openapi.yaml`, applies the goose migrations in `migrations/` at startup, and embeds the built Web Client and docs (`static/`, filled by `mise run server:build`).

```bash
mise run server:build      # web + docs, then the release binary at bin/server
mise run server:test       # unit tests and the contract tests (tests/contract.rs)
mise run server:check      # rustfmt, clippy, sqlx offline query data
mise run server:prepare    # regenerate .sqlx after changing a query or migration
```

`cargo run -p earthly-music-server` runs it in development; `src/bin/repair-artist-credits.rs` is the Artist credit repair command (`docs/operations/repair-artist-credits.md`).

## Layout

```text
src/main.rs, app.rs     config, logging, router, middleware
src/db.rs, migrate.rs   SQLite (WAL, BEGIN IMMEDIATE writes), goose-compatible migrator
src/respond.rs          ApiError and the JSON error envelope
src/inspect/            Strict Import Profile media inspection (symphonia; ffmpeg for Opus and M4A)
src/modules/<module>/   one module per domain: routes/handlers, service, repository
testdata/               audio fixtures shared with the Web E2E and Desktop import parity tests
```

`tests/search_text_parity.rs` recomputes every Library Search text a database holds (`EARTHLY_PARITY_DB=<copy> cargo test -p earthly-music-server --test search_text_parity -- --ignored`).

## External Source (yt-dlp)

The External Source is an optional search of one site through the operator's own [yt-dlp](https://github.com/yt-dlp/yt-dlp); its results play in full through the server and are never stored ([ADR 0032](../docs/adr/0032-external-source-through-an-operator-installed-yt-dlp.md)). It is off by default. The project does not ship yt-dlp: you install it and keep it current.

| Variable | Meaning |
| --- | --- |
| `EXTERNAL_SOURCE_SEARCH` | A yt-dlp search prefix, for example `scsearch` (SoundCloud). Unset, the feature is off and the server never runs yt-dlp. |
| `YT_DLP` | The yt-dlp binary, when it is not on `PATH`. |

You are responsible for what the source is used for and for the site's terms. Choose a source whose content you may play.

### Docker

`server/Dockerfile` builds the Music Server image: the Web Client and docs, the release binary, ffmpeg (Opus and M4A inspection) and the migrations, which the server reads at startup. The build context is the repository root (`docker build -f server/Dockerfile .`, or `docker compose up -d --build` from the root).

The image is built locally and has no yt-dlp by default. The `WITH_YT_DLP` build argument installs it from PyPI into your own image:

```bash
docker build -f server/Dockerfile --build-arg WITH_YT_DLP=true -t earthly-music-server .
```

Then set `EXTERNAL_SOURCE_SEARCH` at run time. To update yt-dlp, rebuild with `--no-cache`, or run `/opt/yt-dlp/bin/pip install -U yt-dlp` in the container.

### Publishing an image

An image pushed to a registry is a release of the project, so:

- Build it with the default `WITH_YT_DLP=false`. A published image never contains yt-dlp, and there is no `with-yt-dlp` tag.
- Keep secrets such as `LASTFM_API_KEY` out of the image; pass them at run time.
- Tag it with the release version, not only `latest`.
- Keep ffmpeg's license notices; the image redistributes Alpine's ffmpeg package.

An operator who uses a published image and wants the External Source derives their own image:

```dockerfile
FROM <registry>/earthly-music-server:<version>
RUN apk add --no-cache python3 \
    && python3 -m venv /opt/yt-dlp \
    && /opt/yt-dlp/bin/pip install --no-cache-dir yt-dlp \
    && ln -s /opt/yt-dlp/bin/yt-dlp /usr/local/bin/yt-dlp
ENV EXTERNAL_SOURCE_SEARCH=scsearch
```

Or they install yt-dlp into a mounted volume and point `YT_DLP` at it.
