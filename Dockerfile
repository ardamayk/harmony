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
