#!/usr/bin/env bash
set -euo pipefail

quality_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
quality_tmp="$(mktemp -d)"

# shellcheck source=scripts/docker-test-lib.sh
. "$quality_repo_root/scripts/docker-test-lib.sh"

cleanup_quality_gate() {
  local exit_status="$?"
  [[ "$quality_tmp" == /tmp/tmp.* ]] && fudian_test_remove_bind_tree "$quality_tmp"
  return "$exit_status"
}
trap cleanup_quality_gate EXIT

cd "$quality_repo_root"

./scripts/check-docs.py
./scripts/check-auth-page-contract.py

fudian_retry_network docker pull postgres:17-alpine >/dev/null
fudian_retry_network docker pull mcr.microsoft.com/playwright:v1.62.0-noble >/dev/null
docker build --target development --tag fudian-nextgen-app:latest .
docker build --target runner-runtime --tag fudian-nextgen-runner:latest .
./scripts/build-plugin-images.sh
./scripts/test-plugin-images.sh

# 依赖下载是纯网络阶段：单独重试它，随后的 fmt/clippy/test 一律不重试。
# 这样瞬时网络失败可以恢复，而真实的编译或断言失败不会被重试掩盖。
fudian_retry_network docker run --rm \
  --mount "type=bind,src=$quality_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  fudian-nextgen-app:latest bash -euo pipefail -c 'cargo fetch --locked'

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
./scripts/test-maitu-workflow.sh
./scripts/test-maitu-code-workflow.sh
./scripts/test-goal-http.sh
./scripts/test-context-http.sh
./scripts/test-workspace-runner-http.sh
./scripts/test-scheduler-http.sh
./scripts/test-review-integration-http.sh
SKIP_PLUGIN_IMAGE_SMOKE=1 ./scripts/test-real-plugins-http.sh
./scripts/test-ideas-http.sh
./scripts/test-tooling-http.sh
./scripts/test-inputs-http.sh
./scripts/test-workbench-http.sh
SCREENSHOT_DIR="$quality_tmp/screenshots" ./scripts/test-workbench-browser.sh
SCREENSHOT_DIR="$quality_tmp/screenshots" ./scripts/test-workbench-large.sh
./scripts/test-production-image.sh
./scripts/test-storage-reconciliation.sh
SCREENSHOT_DIR="$quality_tmp/security-screenshots" ./scripts/test-security-https.sh
FUDIAN_RECOVERY_CURRENT_IMAGE=fudian-nextgen-runtime:test \
  ./scripts/test-backup-recovery.sh
FUDIAN_SECURE_COMPOSE_IMAGE=fudian-nextgen-runtime:test \
  ./scripts/test-secure-compose.sh

./scripts/check-secrets.py

git diff --check
echo "complete quality gate passed"
