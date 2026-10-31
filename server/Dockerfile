FROM node:24-alpine AS frontend-builder
WORKDIR /src
RUN apk add --no-cache bash && npm install --global pnpm@12.13.0
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml vite.config.ts ./
COPY server ./server
COPY packages ./packages
COPY web ./web
COPY desktop/package.json ./desktop/package.json
COPY scripts ./scripts
RUN pnpm install --frozen-lockfile --ignore-scripts
RUN pnpm --dir web run build \
    && pnpm --dir packages/docs run build \
    && bash scripts/sync-static.sh

FROM rust:1.96-alpine AS builder
WORKDIR /src
RUN apk add --no-cache musl-dev
COPY Cargo.toml Cargo.lock ./
# The workspace includes desktop/core; only the Music Server is built.
COPY desktop/core ./desktop/core
COPY packages/contracts/openapi.yaml ./packages/contracts/openapi.yaml
COPY --from=frontend-builder /src/server ./server
# Kept between builds, so a change rebuilds the server and not every dependency.
# The binary leaves the cache here, and touching the embedding file picks up a
# Web Client change that no Rust file shows.
RUN --mount=type=cache,id=harmony-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=harmony-cargo-target,target=/src/target,sharing=locked \
    sh -c 'touch server/src/static_assets.rs \
        && SQLX_OFFLINE=true cargo build --release -p earthly-music-server \
        && cp target/release/earthly-music-server /usr/local/bin/earthly-music-server'

FROM alpine:3.21
RUN apk add --no-cache ca-certificates ffmpeg
# The External Source's yt-dlp (ADR 0032) is the operator's own install: only a
# local build with --build-arg WITH_YT_DLP=true adds it, from PyPI. Published
# images are built without it.
ARG WITH_YT_DLP=false
RUN if [ "$WITH_YT_DLP" = "true" ]; then \
        apk add --no-cache python3 \
        && python3 -m venv /opt/yt-dlp \
        && /opt/yt-dlp/bin/pip install --no-cache-dir yt-dlp \
        && ln -s /opt/yt-dlp/bin/yt-dlp /usr/local/bin/yt-dlp; \
    fi
WORKDIR /app
COPY --from=builder /usr/local/bin/earthly-music-server /app/server
COPY --from=builder /src/server/migrations /app/migrations
ENV SERVER_ADDR=127.0.0.1:8090
ENV DATABASE_PATH=/data/app.db
# Imported music lives in the data volume, next to the database.
ENV MANAGED_STORAGE_PATH=/data/managed
EXPOSE 8090
VOLUME ["/data", "/music"]
ENTRYPOINT ["/app/server"]
