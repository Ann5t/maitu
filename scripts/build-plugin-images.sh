#!/usr/bin/env bash
set -euo pipefail

plugin_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

docker build -f "$plugin_repo_root/plugins/Dockerfile" --target rust-plugin \
  --tag fudian-plugin-rust:1.0.0 "$plugin_repo_root/plugins"
docker build -f "$plugin_repo_root/plugins/Dockerfile" --target cxx-plugin \
  --tag fudian-plugin-cxx:1.0.0 "$plugin_repo_root/plugins"
docker build -f "$plugin_repo_root/plugins/Dockerfile" --target python-v1-plugin \
  --tag fudian-plugin-python:1.0.0 "$plugin_repo_root/plugins"
docker build -f "$plugin_repo_root/plugins/Dockerfile" --target python-v2-plugin \
  --tag fudian-plugin-python:2.0.0 "$plugin_repo_root/plugins"
docker build -f "$plugin_repo_root/plugins/Dockerfile" --target playwright-plugin \
  --tag fudian-plugin-playwright:1.0.0 "$plugin_repo_root/plugins"

echo "real plugin images built: Rust, C/C++, Python v1/v2 and Playwright"
