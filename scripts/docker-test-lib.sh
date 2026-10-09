#!/usr/bin/env bash

fudian_test_open_runner_output() {
  local container="$1"
  local output_key="$2"

  if [[ ! "$output_key" =~ ^jobs/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; then
    echo "refusing unsafe Runner output key: $output_key" >&2
    return 64
  fi

  docker exec --user 0:0 "$container" \
    chmod 0777 -- "/data/runner/$output_key"
}

fudian_test_write_managed_file() {
  local container="$1"
  local path="$2"
  local content="$3"

  case "$path" in
    /data/runner/jobs/* | /data/worktrees/projects/*) ;;
    *)
      echo "refusing unmanaged container path: $path" >&2
      return 64
      ;;
  esac
  if [[ "$path" == *'/../'* || "$path" == */.. ]]; then
    echo "refusing unsafe container path: $path" >&2
    return 64
  fi

  docker exec --user 0:0 "$container" \
    sh -c 'printf %s "$1" > "$2"' _ "$content" "$path"
}

# 网络阶段的有界重试。完整质量门在 CI 上每次都要冷下载镜像与 Rust 依赖，
# 一次瞬时失败会让与提交内容无关的分支变红（纯文档分支曾以 exit 101 失败）。
# 只用于下载类命令；断言与测试一律不重试，避免掩盖真实失败。
fudian_retry_network() {
  local attempts="${FUDIAN_NETWORK_ATTEMPTS:-3}"
  local delay="${FUDIAN_NETWORK_DELAY_SECONDS:-10}"
  local attempt=1
  local status=0

  while true; do
    status=0
    "$@" || status=$?
    if [[ "$status" -eq 0 ]]; then
      return 0
    fi
    if [[ "$attempt" -ge "$attempts" ]]; then
      echo "network step failed after $attempt attempt(s), exit $status: $*" >&2
      return "$status"
    fi
    echo "network step failed (attempt $attempt/$attempts, exit $status); retrying in ${delay}s: $*" >&2
    sleep "$delay"
    attempt=$((attempt + 1))
  done
}

fudian_test_remove_bind_tree() {
  local tree="$1"
  local foreign_entry

  [[ -e "$tree" ]] || return 0
  if [[ "$tree" != /tmp/tmp.* ]]; then
    echo "refusing unsafe test cleanup path: $tree" >&2
    return 64
  fi

  foreign_entry="$(find "$tree" -xdev ! -uid "$(id -u)" -print -quit 2>/dev/null || true)"
  if [[ -n "$foreign_entry" ]]; then
    docker run --rm \
      --network none \
      --read-only \
      --user 0:0 \
      --security-opt no-new-privileges:true \
      --mount "type=bind,src=$tree,dst=/cleanup" \
      --entrypoint chmod \
      "${FUDIAN_TEST_PERMISSION_IMAGE:-postgres:17-alpine}" \
      -R a+rwX /cleanup >/dev/null 2>&1 || true
  fi

  rm -rf -- "$tree"
}
