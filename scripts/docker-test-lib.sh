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
