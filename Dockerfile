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

FROM golang:1.26.9-alpine AS builder
WORKDIR /src
RUN apk add --no-cache git
COPY go.work ./
COPY --from=frontend-builder /src/server ./server
WORKDIR /src/server
RUN go mod download && CGO_ENABLED=0 go build -o /server ./cmd/server

FROM alpine:3.21
RUN apk add --no-cache ca-certificates ffmpeg
WORKDIR /app
COPY --from=builder /server /app/server
ENV SERVER_ADDR=127.0.0.1:8090
ENV DATABASE_PATH=/data/app.db
EXPOSE 8090
VOLUME ["/data", "/music"]
ENTRYPOINT ["/app/server"]
