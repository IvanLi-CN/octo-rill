# syntax=docker/dockerfile:1

ARG BUN_VERSION=1.3.9
ARG RUST_VERSION=1.91.0
ARG DEBIAN_VERSION=bookworm

FROM oven/bun:${BUN_VERSION} AS web-builder
WORKDIR /app/web

COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile

COPY web/ ./
RUN bun run build

FROM rust:${RUST_VERSION}-slim-${DEBIAN_VERSION} AS rust-builder
RUN apt-get update \
  && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY --from=web-builder /app/web/dist ./web/dist

RUN cargo build --release --locked

FROM debian:${DEBIAN_VERSION}-slim AS runner
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder /app/target/release/octo-rill /app/octo-rill
COPY --from=web-builder /app/web/dist /app/web/dist

ENV OCTORILL_BIND_ADDR=0.0.0.0:3000
EXPOSE 3000

CMD ["/app/octo-rill"]

