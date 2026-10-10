# syntax=docker/dockerfile:1

FROM rust:1.97-slim@sha256:5c6f46a6e4472ab1ca7ba7d494e6677f2f219ebc02f32025d3986f057635ec9c AS toolchain
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

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
RUN --mount=type=cache,id=maitu-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=maitu-cargo-git,target=/usr/local/cargo/git \
    CARGO_HTTP_MULTIPLEXING=false cargo build --release --locked

FROM debian:trixie-slim@sha256:3a39a0592364683e6bab97937b72cad5a8fa6dcbbee90edb3bb48c7f8e94f258 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home fudian \
    && mkdir -p /app/assets /data/artifacts \
    && chown -R fudian:fudian /app /data/artifacts
WORKDIR /app
COPY --from=builder /app/target/release/fudian /usr/local/bin/fudian
COPY --from=builder /app/target/release/fudian-maintenance /usr/local/bin/fudian-maintenance
COPY --from=builder /app/target/release/maitu-check-worker /usr/local/bin/maitu-check-worker
COPY --from=builder /app/target/release/maitu-review-worker /usr/local/bin/maitu-review-worker
COPY --chown=fudian:fudian assets ./assets
# 构建来源修订：备份元数据用它证明"归档的源码"与"运行中的镜像"同源。
# 放在最后的层，改动它不会让前面的编译缓存失效。
ARG FUDIAN_SOURCE_REVISION=unknown
LABEL org.opencontainers.image.revision=$FUDIAN_SOURCE_REVISION
USER fudian
ENV FUDIAN_BIND=0.0.0.0:3000 \
    ARTIFACT_ROOT=/data/artifacts \
    RUST_LOG=fudian=info,tower_http=info
EXPOSE 3000
CMD ["fudian"]

FROM debian:trixie-slim@sha256:3a39a0592364683e6bab97937b72cad5a8fa6dcbbee90edb3bb48c7f8e94f258 AS runner-runtime
RUN useradd --system --uid 1000 --create-home runner \
    && mkdir -p /workspace/input /workspace/output /workspace/result /tmp/fudian-home \
    && chown -R runner:runner /workspace/output /workspace/result /tmp/fudian-home
COPY --from=builder /app/target/release/fudian-runner /usr/local/bin/fudian-runner
COPY --from=builder /app/target/release/fudian-tool-runtime /opt/fudian/fudian-tool-runtime
USER runner
WORKDIR /workspace/input
ENTRYPOINT ["/usr/local/bin/fudian-runner"]

FROM toolchain AS code-runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs npm python3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home checker \
    && mkdir -p /input
COPY --from=builder /app/target/release/maitu-code-check /usr/local/bin/maitu-code-check
USER checker
WORKDIR /input
ENTRYPOINT ["/usr/local/bin/maitu-code-check"]
