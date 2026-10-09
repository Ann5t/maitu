#!/usr/bin/env bash
# 依赖安全审计：用 RustSec 咨询库检查 Cargo.lock 里的全部依赖。
#
# 故意不接入 scripts/quality-gate.sh。咨询库每天更新，接进必过的质量门会让
# 一个分支的成败取决于提交之外的第三方数据：新披露的咨询可以让本来绿色的
# 分支隔夜变红，而那一次提交并没有改任何依赖。需要阻断时由维护者显式运行本脚本。
#
# 需要网络：首次会在专用卷里编译 cargo-audit（约 1 分钟），之后复用；
# 退出码沿用 cargo-audit：发现漏洞时为 1。
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tools_volume="${FUDIAN_AUDIT_TOOLS_VOLUME:-fudian_audit_tools}"
image="${FUDIAN_DEVELOPMENT_IMAGE:-fudian-nextgen-app:latest}"

docker volume create "$tools_volume" >/dev/null

docker run --rm \
  --mount "type=bind,src=$repo_root,dst=/app" \
  --mount "type=volume,src=$tools_volume,dst=/audit-tools" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  "$image" bash -euo pipefail -c '
    export PATH=/audit-tools/bin:$PATH
    if ! command -v cargo-audit >/dev/null 2>&1; then
      echo "首次运行：编译 cargo-audit 到 /audit-tools" >&2
      cargo install --locked --root /audit-tools cargo-audit >&2
    fi
    cd /app
    cargo-audit audit
  '