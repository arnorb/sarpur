# The sarpur image: the server and ffmpeg.
# Runs as 568:568 under tini; data in /data, the library mounted by the user
# (on the NAS: /mnt/storage/Media at /media). See docs/docker.md.

# 1. The UI.
FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# 2. The server, with the UI embedded. glibc, like the runtime image.
FROM rust:1-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked -p sarpur \
    && cp target/release/sarpur /sarpur

# 3. The runtime.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates ffmpeg tini tzdata \
    && rm -rf /var/lib/apt/lists/*
COPY LICENSE /app/LICENSE
COPY --from=server /sarpur /app/sarpur

ENV SARPUR_DATA_DIR=/data \
    SARPUR_BIND=0.0.0.0 \
    SARPUR_PORT=8090 \
    HOME=/data \
    TZ=Atlantic/Reykjavik

RUN mkdir -p /data /media && chown 568:568 /data /media
VOLUME /data
USER 568:568
EXPOSE 8090
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --start-interval=2s --retries=3 \
    CMD ["/app/sarpur", "--healthcheck"]
ENTRYPOINT ["/usr/bin/tini", "--", "/app/sarpur"]
