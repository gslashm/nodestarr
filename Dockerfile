# Nodestarr container image.
#
#   docker build -t nodestarr .
#
# Three stages: the web UI is built with Node, then embedded into a static
# (musl) Rust binary, which runs on a small Alpine image together with the
# Docker CLI and its Compose plugin (Nodestarr drives stacks through them).
#
# Build arguments:
#   NODESTARR_REPO  GitHub "owner/name" that publishes Nodestarr releases. Turns on
#                   "new Nodestarr version" checks and release notes; leave
#                   empty for local builds.

# ---------------------------------------------------------------------------
# 1. Web UI
# ---------------------------------------------------------------------------
FROM node:22-alpine AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ---------------------------------------------------------------------------
# 2. Server binary
# ---------------------------------------------------------------------------
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev gcc
WORKDIR /src

# Build the dependencies first, against placeholder sources, so this layer
# stays cached until Cargo.toml or Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
COPY --from=web /src/web/dist ./web/dist
ARG NODESTARR_REPO=""
# Touch the entry points so Cargo rebuilds them instead of reusing the
# placeholder build above.
RUN touch src/main.rs src/lib.rs \
    && NODESTARR_REPO="${NODESTARR_REPO}" cargo build --release --locked

# ---------------------------------------------------------------------------
# 3. Runtime
# ---------------------------------------------------------------------------
FROM alpine:3.22
RUN apk add --no-cache ca-certificates tzdata docker-cli docker-cli-compose
COPY --from=build /src/target/release/nodestarr /usr/local/bin/nodestarr

ENV NODESTARR_DATA_DIR=/config \
    NODESTARR_LISTEN=0.0.0.0:7979
VOLUME /config
EXPOSE 7979

HEALTHCHECK --interval=60s --timeout=10s --start-period=20s --retries=3 \
    CMD ["nodestarr", "healthcheck"]

LABEL org.opencontainers.image.title="Nodestarr" \
      org.opencontainers.image.description="Monitor, update and back up the Docker containers and Compose stacks on your server." \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"

ENTRYPOINT ["nodestarr"]
