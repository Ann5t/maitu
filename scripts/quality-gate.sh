#!/usr/bin/env bash
set -euo pipefail

quality_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
quality_tmp="$(mktemp -d)"

cleanup_quality_gate() {
  local exit_status="$?"
  [[ "$quality_tmp" == /tmp/tmp.* ]] && rm -rf "$quality_tmp"
  return "$exit_status"
}
trap cleanup_quality_gate EXIT

cd "$quality_repo_root"

docker pull postgres:17-alpine >/dev/null
docker pull mcr.microsoft.com/playwright:v1.62.0-noble >/dev/null
docker build --target development --tag fudian-nextgen-app:latest .
docker build --target runner-runtime --tag fudian-nextgen-runner:latest .

docker run --rm \
  --mount "type=bind,src=$quality_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest bash -euo pipefail -c '
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets --all-features -- -D warnings
    cargo test --locked --all-targets
  '

./scripts/test-goal-migrations.sh
./scripts/test-goal-http.sh
./scripts/test-context-http.sh
./scripts/test-workspace-runner-http.sh
./scripts/test-ideas-http.sh
./scripts/test-tooling-http.sh
./scripts/test-inputs-http.sh
./scripts/test-workbench-http.sh
SCREENSHOT_DIR="$quality_tmp/screenshots" ./scripts/test-workbench-browser.sh
./scripts/test-production-image.sh

git diff --check
echo "complete quality gate passed"
