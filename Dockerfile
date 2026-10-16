FROM oven/bun:1.4.2-alpine AS frontend-builder
WORKDIR /src
RUN apk add --no-cache bash nodejs
COPY package.json bun.lock vite.config.ts ./
COPY server ./server
COPY packages ./packages
COPY web ./web
COPY desktop/package.json ./desktop/package.json
COPY scripts ./scripts
RUN bun install --frozen-lockfile --ignore-scripts
RUN bun run --cwd web build \
    && bun run --cwd packages/docs build \
    && bash scripts/sync-static.sh

FROM rust:1.96-alpine AS builder
WORKDIR /src
RUN apk add --no-cache musl-dev
COPY Cargo.toml Cargo.lock ./
# The workspace includes desktop/core; only the Music Server is built.
COPY desktop/core ./desktop/core
COPY packages/contracts/openapi.yaml ./packages/contracts/openapi.yaml
COPY --from=frontend-builder /src/server ./server
RUN SQLX_OFFLINE=true cargo build --release -p earthly-music-server

FROM alpine:3.21
RUN apk add --no-cache ca-certificates ffmpeg
WORKDIR /app
COPY --from=builder /src/target/release/earthly-music-server /app/server
COPY --from=builder /src/server/migrations /app/migrations
ENV SERVER_ADDR=127.0.0.1:8090
ENV DATABASE_PATH=/data/app.db
EXPOSE 8090
VOLUME ["/data", "/music"]
ENTRYPOINT ["/app/server"]
