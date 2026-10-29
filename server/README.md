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

## Soulseek Acquisition (slskd)

Soulseek Acquisition downloads a chosen Acquisition Version through the operator's own [slskd](https://github.com/slskd/slskd) and hands the files to Managed Import ([ADR 0036](../docs/adr/0036-soulseek-acquisition-through-an-operator-run-slskd.md)). It is off by default. The project does not ship or install slskd: you run it with your own Soulseek account, which stays in slskd's configuration, and you are responsible for what is downloaded and shared.

| Variable | Meaning |
| --- | --- |
| `SLSKD_URL` | slskd's http or https address, for example `http://127.0.0.1:5030`. |
| `SLSKD_API_KEY` | An slskd API key with the `ReadWrite` role (searches and transfers; the status route also reads slskd's shares). |
| `SLSKD_DOWNLOADS_PATH` | slskd's completed downloads directory as the Music Server sees it. |

Set all three to turn the feature on, or none to leave it off; the server refuses to start with only some of them, or when `SLSKD_DOWNLOADS_PATH` is missing or unreadable.

### Running slskd

1. Install and run slskd yourself, following [its documentation](https://github.com/slskd/slskd) (a release binary or the `slskd/slskd` image). Put your Soulseek user name and password in slskd's configuration (`soulseek.username`, `soulseek.password`). The Music Server never sees them; it only asks slskd whether it is logged in.
2. Give the Music Server an API key with the `ReadWrite` role: add one under `web.authentication.api_keys` in `slskd.yml`, or start slskd with `--api-key`. Set the same key as `SLSKD_API_KEY`, and slskd's web address as `SLSKD_URL`.
3. Set `SLSKD_DOWNLOADS_PATH` to slskd's downloads directory (`directories.downloads`) as the Music Server sees it: the same directory on the same host, or one volume mounted into both containers. The Music Server reads finished files from there and deletes each copy once Managed Import has it, or when the acquisition is cancelled, so its user needs write access too.
4. Settings › Soulseek shows whether slskd is reachable and logged in.

The Music Server expects each finished file at `<downloads>/<last folder of the remote path>/<file name>`, slskd's default download layout (`${SOURCE_DIRECTORY}`). If you change slskd's download subdirectory setting, or slskd renames a file because one with the same name is already there, the server cannot find the file: that source then counts as stalled after two minutes without progress, and the acquisition moves on to the next source. Partial files stay in slskd's incomplete directory (`directories.incomplete`); slskd manages them, the Music Server does not touch them.

### Soulseek Sharing

The Music Server does not share anything itself. To share your library on Soulseek, give slskd read-only access to Managed Storage (`MANAGED_STORAGE_PATH`) at the same absolute path the Music Server uses, and list that path, or a directory above it, in slskd's `shares.directories`. The server compares slskd's shares with that path, so a different mount point reads as not shared, and a share of only a subdirectory does not count. slskd shares newly imported Tracks after its next share scan. Settings › Soulseek only shows the state; there is no switch.

### Docker Compose

slskd runs beside the Music Server as its own service. A sketch for the root `docker-compose.yml`, with slskd's downloads in a volume both services mount and Managed Storage (`/data/managed` in the `app-data` volume) mounted read-only into slskd at the same path. The `subpath` option needs Docker Engine 26 or later; on older engines bind-mount a host directory into both services instead.

```yaml
services:
  server:
    # ...as in docker-compose.yml, plus:
    environment:
      SLSKD_URL: http://127.0.0.1:5030
      SLSKD_API_KEY: ${SLSKD_API_KEY}
      SLSKD_DOWNLOADS_PATH: /downloads
    volumes:
      - app-data:/data
      - slskd-downloads:/downloads # read and write: the server deletes imported copies

  slskd:
    image: slskd/slskd
    ports:
      - "127.0.0.1:5030:5030" # web UI and API, reached by the server on the host network
      - "50300:50300" # Soulseek listen port
    volumes:
      - slskd-app:/app # slskd.yml: Soulseek account, API key, shares
      - slskd-downloads:/downloads
      - type: volume # optional, for Soulseek Sharing
        source: app-data
        target: /data/managed
        read_only: true
        volume:
          subpath: managed

volumes:
  app-data:
  slskd-app:
  slskd-downloads:
```

and in slskd's `slskd.yml` (in the `slskd-app` volume):

```yaml
soulseek:
  username: your-soulseek-user
  password: your-soulseek-password
directories:
  downloads: /downloads
shares:
  directories:
    - /data/managed # only to share Managed Storage
web:
  authentication:
    api_keys:
      earthly_music_server:
        key: a-long-random-string # the same value as SLSKD_API_KEY
        role: readwrite
```

Both services must be able to write to the downloads volume; if slskd and the Music Server run as different users, give that directory to a group both belong to.

### Responsibility

Downloading or sharing copyrighted music without the rights holder's permission is unlawful in many places. You run slskd, you choose what it downloads and shares, and you are responsible for both; the project ships no slskd and enables nothing by default.

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
