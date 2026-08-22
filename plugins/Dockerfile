# syntax=docker/dockerfile:1

FROM fudian-nextgen-runner:latest AS fudian-payload

FROM rust:1.97-slim AS rust-plugin
RUN if ! getent passwd 1000 >/dev/null; then useradd --system --uid 1000 --create-home plugin; fi \
    && mkdir -p /workspace/input /workspace/output /workspace/result /tmp/fudian-home \
    && chown -R 1000:1000 /workspace/output /workspace/result /tmp/fudian-home
COPY --from=fudian-payload /usr/local/bin/fudian-runner /usr/local/bin/fudian-runner
COPY --from=fudian-payload /opt/fudian/fudian-tool-runtime /runtime/fudian-tool-runtime
USER 1000
WORKDIR /workspace/input
ENTRYPOINT ["/usr/local/bin/fudian-runner"]

FROM gcc:15-trixie AS cxx-plugin
RUN if ! getent passwd 1000 >/dev/null; then useradd --system --uid 1000 --create-home plugin; fi \
    && mkdir -p /workspace/input /workspace/output /workspace/result /tmp/fudian-home \
    && chown -R 1000:1000 /workspace/output /workspace/result /tmp/fudian-home
COPY --from=fudian-payload /usr/local/bin/fudian-runner /usr/local/bin/fudian-runner
COPY --from=fudian-payload /opt/fudian/fudian-tool-runtime /runtime/fudian-tool-runtime
USER 1000
WORKDIR /workspace/input
ENTRYPOINT ["/usr/local/bin/fudian-runner"]

FROM python:3.13-slim-trixie AS python-base
RUN if ! getent passwd 1000 >/dev/null; then useradd --system --uid 1000 --create-home plugin; fi \
    && mkdir -p /workspace/input /workspace/output /workspace/result /tmp/fudian-home \
    && chown -R 1000:1000 /workspace/output /workspace/result /tmp/fudian-home
COPY --from=fudian-payload /usr/local/bin/fudian-runner /usr/local/bin/fudian-runner
COPY --from=fudian-payload /opt/fudian/fudian-tool-runtime /runtime/fudian-tool-runtime
USER 1000
WORKDIR /workspace/input
ENTRYPOINT ["/usr/local/bin/fudian-runner"]

FROM python-base AS python-v1-plugin
COPY --chown=1000:1000 python/v1/fudian_example.py /usr/local/lib/python3.13/site-packages/fudian_example.py

FROM python-base AS python-v2-plugin
COPY --chown=1000:1000 python/v2/fudian_example.py /usr/local/lib/python3.13/site-packages/fudian_example.py

FROM mcr.microsoft.com/playwright:v1.62.0-noble AS playwright-plugin
RUN npm install --prefix /opt/fudian-playwright --no-audit --no-fund playwright@1.62.0 \
    && if ! getent passwd 1000 >/dev/null; then useradd --system --uid 1000 --create-home plugin; fi \
    && mkdir -p /workspace/input /workspace/output /workspace/result /tmp/fudian-home \
    && chown -R 1000:1000 /workspace/output /workspace/result /tmp/fudian-home
COPY --from=fudian-payload /usr/local/bin/fudian-runner /usr/local/bin/fudian-runner
COPY --from=fudian-payload /opt/fudian/fudian-tool-runtime /runtime/fudian-tool-runtime
COPY --chown=1000:1000 playwright/inspect.mjs playwright/version.mjs /opt/fudian-playwright/
USER 1000
WORKDIR /workspace/input
ENTRYPOINT ["/usr/local/bin/fudian-runner"]
