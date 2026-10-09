#!/usr/bin/env bash
set -Eeuo pipefail

# 门在 CI 上要跑二十多分钟，失败时必须能一眼看出是哪一步、哪一行、哪条命令。
quality_step="启动"
quality_reported=""
step() {
  quality_step="$1"
  printf '==> %s\n' "$1"
}
trap 'quality_status=$?; if [[ -z "$quality_reported" ]]; then quality_reported=1; printf "\n质量门失败：步骤「%s」在 quality-gate.sh 第 %s 行以退出码 %s 结束\n  失败命令：%s\n" "$quality_step" "$LINENO" "$quality_status" "$BASH_COMMAND" >&2; fi' ERR
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

step "检查文档一致性"
./scripts/check-docs.py

step "拉取基础镜像"
docker pull postgres:17-alpine >/dev/null
docker pull mcr.microsoft.com/playwright:v1.62.0-noble >/dev/null
step "构建开发镜像与插件镜像"
docker build --target development --tag fudian-nextgen-app:latest .
docker build --target runner-runtime --tag fudian-nextgen-runner:latest .
./scripts/build-plugin-images.sh
./scripts/test-plugin-images.sh

step "Rust 格式、lint 与单元测试"
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

step "隔离数据库迁移与工作流测试"
./scripts/test-goal-migrations.sh
./scripts/test-maitu-workflow.sh
./scripts/test-maitu-code-workflow.sh
step "隔离 HTTP 接口测试"
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
step "浏览器端到端测试"
SCREENSHOT_DIR="$quality_tmp/screenshots" ./scripts/test-workbench-browser.sh
SCREENSHOT_DIR="$quality_tmp/screenshots" ./scripts/test-workbench-large.sh
step "生产镜像与存储一致性"
./scripts/test-production-image.sh
./scripts/test-storage-reconciliation.sh
step "私有 HTTPS 安全测试"
SCREENSHOT_DIR="$quality_tmp/security-screenshots" ./scripts/test-security-https.sh
step "备份恢复演练"
FUDIAN_RECOVERY_CURRENT_IMAGE=fudian-nextgen-runtime:test \
  ./scripts/test-backup-recovery.sh
step "安全 Compose 测试"
FUDIAN_SECURE_COMPOSE_IMAGE=fudian-nextgen-runtime:test \
  ./scripts/test-secure-compose.sh

step "Git 空白检查与收尾"
git diff --check
echo "complete quality gate passed"
