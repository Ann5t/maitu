#!/usr/bin/env bash
set -euo pipefail
umask 077

restore_bundle="${1:?usage: restore-v2.sh <verified-backup-directory>}"
restore_database_container="${FUDIAN_RESTORE_DATABASE_CONTAINER:?set FUDIAN_RESTORE_DATABASE_CONTAINER explicitly}"
restore_database_user="${FUDIAN_RESTORE_POSTGRES_USER:-fudian}"
restore_database_name="${FUDIAN_RESTORE_POSTGRES_DB:-fudian}"
restore_artifact_volume="${FUDIAN_RESTORE_ARTIFACT_VOLUME:?set FUDIAN_RESTORE_ARTIFACT_VOLUME explicitly}"
restore_repository_volume="${FUDIAN_RESTORE_REPOSITORY_VOLUME:?set FUDIAN_RESTORE_REPOSITORY_VOLUME explicitly}"
restore_worktree_volume="${FUDIAN_RESTORE_WORKTREE_VOLUME:?set FUDIAN_RESTORE_WORKTREE_VOLUME explicitly}"
restore_runner_volume="${FUDIAN_RESTORE_RUNNER_VOLUME:?set FUDIAN_RESTORE_RUNNER_VOLUME explicitly}"
restore_git_image="${FUDIAN_RESTORE_APP_IMAGE:?set FUDIAN_RESTORE_APP_IMAGE to the reviewed Fudian runtime image}"
restore_archive_image="postgres:17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73"

if [[ "${FUDIAN_RESTORE_CONFIRM:-}" != "EMPTY_LABELED_TARGETS" ]]; then
  echo "恢复仅允许写入带专用标签的空目标；请显式设置 FUDIAN_RESTORE_CONFIRM=EMPTY_LABELED_TARGETS" >&2
  exit 1
fi
restore_bundle="$(cd "$restore_bundle" && pwd)"
(cd "$restore_bundle" && sha256sum --check --strict SHA256SUMS)

validate_docker_name() {
  local label="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]]; then
    echo "$label 不是明确的 Docker 名称：$value" >&2
    exit 1
  fi
}

require_restore_label() {
  local object_kind="$1"
  local object_name="$2"
  local label
  if [[ "$object_kind" == volume ]]; then
    label="$(docker volume inspect -f '{{index .Labels "com.fudian.restore-target"}}' "$object_name" 2>/dev/null || true)"
  else
    label="$(docker container inspect -f '{{index .Config.Labels "com.fudian.restore-target"}}' "$object_name" 2>/dev/null || true)"
  fi
  if [[ "$label" != true ]]; then
    echo "拒绝恢复：$object_kind $object_name 缺少 com.fudian.restore-target=true" >&2
    exit 1
  fi
}

for restore_name in "$restore_database_container" "$restore_artifact_volume" \
  "$restore_repository_volume" "$restore_worktree_volume" "$restore_runner_volume"; do
  validate_docker_name "恢复目标" "$restore_name"
done
require_restore_label container "$restore_database_container"
for restore_volume in "$restore_artifact_volume" "$restore_repository_volume" \
  "$restore_worktree_volume" "$restore_runner_volume"; do
  require_restore_label volume "$restore_volume"
  docker run --rm --network none --read-only \
    --mount "type=volume,src=$restore_volume,dst=/target" \
    "$restore_archive_image" \
    sh -ec 'test -z "$(find /target -mindepth 1 -maxdepth 1 -print -quit)"'
done

restore_public_tables="$(docker exec "$restore_database_container" \
  psql --username="$restore_database_user" --dbname="$restore_database_name" \
  --no-align --tuples-only --set=ON_ERROR_STOP=1 \
  --command="SELECT count(*) FROM pg_tables WHERE schemaname = 'public';")"
if [[ "$restore_public_tables" != 0 ]]; then
  echo "拒绝恢复：目标数据库 public schema 不是空的" >&2
  exit 1
fi

validate_archive() {
  local archive="$1"
  if tar -tzf "$restore_bundle/$archive" \
    | awk 'BEGIN { bad=0 } /^\// { bad=1 } /(^|\/)\.\.($|\/)/ { bad=1 } END { exit bad ? 0 : 1 }'; then
    echo "归档包含不安全路径：$archive" >&2
    exit 1
  fi
}

restore_volume() {
  local volume="$1"
  local archive="$2"
  local file_manifest="$3"
  validate_archive "$archive"
  docker run --rm --network none --read-only \
    --mount "type=volume,src=$volume,dst=/target" \
    --mount "type=bind,src=$restore_bundle,dst=/backup,readonly" \
    "$restore_archive_image" \
    sh -ec "tar -xzf '/backup/$archive' -C /target"
  docker run --rm --network none --read-only \
    --mount "type=volume,src=$volume,dst=/target,readonly" \
    "$restore_archive_image" \
    sh -ec 'cd /target && find . -type f -print0 | sort -z | xargs -0 -r sha256sum' \
    | diff -u "$restore_bundle/$file_manifest" -
}

echo "恢复数据库到专用空目标：$restore_database_container"
docker exec -i "$restore_database_container" \
  pg_restore --username="$restore_database_user" --dbname="$restore_database_name" \
  --no-owner --no-privileges --exit-on-error <"$restore_bundle/database.dump"

restore_volume "$restore_artifact_volume" artifacts.tar.gz artifacts.files.sha256
restore_volume "$restore_repository_volume" repositories.tar.gz repositories.files.sha256
restore_volume "$restore_worktree_volume" worktrees.tar.gz worktrees.files.sha256
restore_volume "$restore_runner_volume" runner-outputs.tar.gz runner-outputs.files.sha256

docker run --rm --network none --read-only \
  --mount "type=volume,src=$restore_repository_volume,dst=/repositories,readonly" \
  --entrypoint sh "$restore_git_image" -ec '
    found=0
    for repository in /repositories/projects/*.git; do
      [ -d "$repository" ] || continue
      found=1
      git --git-dir "$repository" fsck --strict
    done
    [ "$found" -eq 1 ] || [ -z "$(find /repositories -mindepth 1 -print -quit)" ]
  '

echo "恢复完成：目标数据库、四类内容卷及 Git 对象均已校验"
