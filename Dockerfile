# syntax=docker/dockerfile:1

FROM rust:1.97-slim AS toolchain
WORKDIR /app

FROM toolchain AS development
RUN rustup component add rustfmt clippy
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY assets ./assets

FROM toolchain AS builder
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home fudian \
    && mkdir -p /app/assets /data/artifacts \
    && chown -R fudian:fudian /app /data/artifacts
WORKDIR /app
COPY --from=builder /app/target/release/fudian /usr/local/bin/fudian
COPY --chown=fudian:fudian assets ./assets
USER fudian
ENV FUDIAN_BIND=0.0.0.0:3000 \
    ARTIFACT_ROOT=/data/artifacts \
    RUST_LOG=fudian=info,tower_http=info
EXPOSE 3000
CMD ["fudian"]
