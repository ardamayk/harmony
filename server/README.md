# Harmony Music Server

The Music Server, written in Rust (crate `earthly-music-server`; axum on tokio, sqlx over SQLite). It serves the HTTP contract in `packages/contracts/openapi.yaml`, applies the goose migrations in `migrations/` at startup, and embeds the built Web Client and docs (`static/`, filled by `mise run server:build`).

```bash
mise run server:dev        # development server (cargo run) on :8090
mise run server:build      # web + docs, then the release binary at bin/server
mise run server:start      # the built bin/server on :8090
mise run server:test       # unit tests and the contract tests (tests/contract.rs)
mise run server:check      # rustfmt, clippy, sqlx offline query data
mise run server:prepare    # regenerate .sqlx after changing a query or migration
```

`server:dev` and `server:start` run it from `server/`, so both use `./data` and `migrations/` here; `src/bin/repair-artist-credits.rs` is the Artist credit repair command (`docs/operations/repair-artist-credits.md`).

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

## Listen Together

Listen Together lets friends join a Room on your Music Server through an Invite Link and listen to your library with you ([ADR 0037](../docs/adr/0037-listen-together-rooms-on-the-owners-music-server.md)). It is off by default. Set `GUEST_ADDR` and `PUBLIC_URL` together, or leave both unset; setting only one stops the server at startup.

| Variable | Meaning |
| --- | --- |
| `GUEST_ADDR` | The Guest Listener's address, for example `127.0.0.1:8091`. It must be a loopback address and differ from `SERVER_ADDR`. |
| `PUBLIC_URL` | The public origin your reverse proxy or tunnel serves, for example `https://music.example.com`, with no path. It must be `https` unless its host is loopback. The Invite Link is built from it, and the Guest Listener answers only this host and origin. |
| `LISTEN_TOGETHER_MAX_GUESTS` | How many Guests a Room admits, 1 to 8. Unset, 8. |
| `LISTEN_TOGETHER_ROOM_HOURS` | How many hours a Room lasts, 1 to 168. Unset, 24. |

**Point your tunnel or reverse proxy at `GUEST_ADDR`, never at `SERVER_ADDR`.** `SERVER_ADDR` has no authentication: it trusts every request because only your own machine can reach it. Exposing it hands your whole Music Server to the internet, including deleting your library. `GUEST_ADDR` serves only the Web Client, joining, and what a Guest may use, and it refuses everything else.

The proxy must terminate HTTPS and pass the public host through in the `Host` header; the Guest Listener refuses any other host. With nginx that means `proxy_set_header Host $http_host;`, which keeps a non-default port (`$host` drops it, and nginx's default sends the upstream's name instead), and it needs `proxy_buffering off` for the Room's event stream. Caddy and `cloudflared` keep the host unless you configure them otherwise.

A Room lives in memory for at most `LISTEN_TOGETHER_ROOM_HOURS`, and a restart ends it. Anyone holding the Invite Link can join until the Room closes, so share it like a password. Each Guest streams from your connection, at the Track's own bitrate.

The Guest Listener answers `429 rate_limited` when a Guest passes a limit; the budgets are constants, listed in ADR 0037, and normal listening stays within them. Reads that run ffmpeg or call an outside service are the strictest, a Guest holds at most 4 audio streams, and a seek is never refused. `POST /api/v1/listen-together/join` has one limit shared by everyone, because the server cannot trust a client address behind a proxy: repeated attempts can delay your friends' joins until they stop.

## Owner access through a signing proxy

The Owner's listener has no sign-in of its own, so it may reach the internet only behind an identity proxy that signs each request with a JWT ([ADR 0038](../docs/adr/0038-owner-access-through-a-signing-proxy.md)). It is off by default. Without it, reach the Owner's listener through an SSH tunnel. Set all five variables or none; setting only some stops the server at startup.

| Variable | Meaning |
| --- | --- |
| `OWNER_PUBLIC_URL` | The Owner's public origin, for example `https://music.example.com`. It must be `https` and differ from `PUBLIC_URL`. |
| `OWNER_JWT_HEADER` | The request header that carries the proxy's token. |
| `OWNER_JWKS_URL` | The proxy's public keys as a JWKS. It must be `https`, or `http` on loopback. |
| `OWNER_JWT_ISSUER` | The token's `iss`. |
| `OWNER_JWT_AUDIENCE` | The token's `aud`. |

Tokens must be signed with RS256 or ES256 (P-256) and carry `exp`, `iss`, `aud` and a `kid` that names a key in the JWKS. The server checks them itself, so a mistake in the proxy's policy does not open your Music Server, and it refuses everything when it cannot fetch the keys. Requests addressed to loopback work as before.

Order matters. Set up the proxy's sign-in first, then these variables, and only then route the public hostname to `SERVER_ADDR`. Reach `SERVER_ADDR` only from the proxy on the same host, for example over a tunnel. Do not put a port forwarder in front of it: a request with a loopback `Host` needs no token, so anyone who reaches the forwarder is the Owner.

For Cloudflare Access with a tunnel, for example, the header is `Cf-Access-Jwt-Assertion`, the keys are at `https://<team>.cloudflareaccess.com/cdn-cgi/access/certs`, the issuer is `https://<team>.cloudflareaccess.com`, and the audience is the application's AUD tag.

## External Source (yt-dlp)

The External Source is an optional search of one site through the operator's own [yt-dlp](https://github.com/yt-dlp/yt-dlp); its results play in full through the server and are never stored ([ADR 0032](../docs/adr/0032-external-source-through-an-operator-installed-yt-dlp.md)). It is off by default. The project does not ship yt-dlp: you install it and keep it current.

| Variable | Meaning |
| --- | --- |
| `EXTERNAL_SOURCE_SEARCH` | A yt-dlp search prefix, for example `scsearch` (SoundCloud). Unset, the feature is off and the server never runs yt-dlp. |
| `YT_DLP` | The yt-dlp binary, when it is not on `PATH`. |
| `EXTERNAL_SOURCE_EXTRACTOR_ARGS` | yt-dlp extractor arguments, passed as `--extractor-args`; separate several with spaces. Unset, none. |
| `EXTERNAL_SOURCE_NAME` | The site's name in the clients, such as `Internet Archive` (1 to 40 printable characters). Unset, a known prefix names it (`ytsearch` YouTube, `scsearch` SoundCloud, `bilisearch` BiliBili, `nicosearch` Niconico, `gvsearch` Google Video, `yvsearch` Yahoo Video, `rkfnsearch` Rokfin), else `External`. |

For example, with `EXTERNAL_SOURCE_SEARCH=ytsearch`, `EXTERNAL_SOURCE_EXTRACTOR_ARGS=youtube:skip=hls,dash` skips YouTube's HLS and DASH manifests the server does not use, so playback starts about 0.3 s sooner. Do not set `player_skip=webpage`: YouTube then asks to sign in to confirm you are not a bot.

You are responsible for what the source is used for and for the site's terms. Choose a source whose content you may play.

### Installing yt-dlp on a host

Install yt-dlp from PyPI with [uv](https://docs.astral.sh/uv/), so it gets its own environment and is easy to update:

```bash
uv tool install yt-dlp     # installs ~/.local/bin/yt-dlp
uv tool upgrade yt-dlp     # when a site changes and searches or playback start failing
```

Put `~/.local/bin` on the server's `PATH`, or set `YT_DLP=$HOME/.local/bin/yt-dlp`. Sites change often, so keep yt-dlp current: an old release can fail on a site that a new one handles (for example, YouTube refusing its stream URLs). The installs yt-dlp itself documents (`pipx`, `pip`, the distribution's package) work the same way. Keep yt-dlp on the Linux filesystem: under WSL, a copy on a Windows drive (`/mnt/c`) starts about a second slower on every search and playback.

### Docker

`server/Dockerfile` builds the Music Server image: the Web Client and docs, the release binary, ffmpeg (Opus and M4A inspection) and the migrations, which the server reads at startup. The build context is the repository root (`docker build -f server/Dockerfile .`, or `docker compose up -d --build` from the root).

`.github/workflows/server-image.yml` publishes it to `ghcr.io/ardamayk/harmony-server`: `main` and `sha-<commit>` after CI passes on main, `<version>` and `latest` from a `v*` tag.

The published image has no yt-dlp, and neither does a local build by default. The `WITH_YT_DLP` build argument installs it from PyPI into your own image:

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

## Sentry (optional)

The Music Server can send errors, request traces, logs and External Source metrics to your own Sentry ([ADR 0033](../docs/adr/0033-opt-in-sentry-for-the-rust-programs.md)). It is off unless `SENTRY_DSN` is set; the project has no DSN of its own and never reports from anyone else's server.

| Variable | Meaning |
| --- | --- |
| `SENTRY_DSN` | Your project's DSN, such as `https://<public-key>@<your-sentry-host>/<project-id>`. Unset, nothing is sent. |
| `SENTRY_ENVIRONMENT` | Unset, `development` for a debug build and `production` for a release build. |
| `SENTRY_TRACES_SAMPLE_RATE` | The share of requests sent as transactions, 0 to 1. Unset, `1.0`. |

Keep the DSN out of git, for example in `mise.local.toml`:

```toml
[env]
SENTRY_DSN = "https://<public-key>@<your-sentry-host>/<project-id>"
SENTRY_ENVIRONMENT = "development"
```

Transactions are named by route template (`GET /api/v1/external/tracks/{external_track_id}/stream`). URLs, query strings, search queries, path ids, headers, client addresses and file paths are not sent; Rust has no Sentry profiling.
