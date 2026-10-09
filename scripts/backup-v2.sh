#!/usr/bin/env bash
set -eEuo pipefail
umask 077
trap 'echo "备份失败：第 $LINENO 行的命令以非零状态退出：$BASH_COMMAND" >&2' ERR

backup_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
backup_parent="${1:-$backup_repo_root/backups}"
backup_timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_database_container="${FUDIAN_BACKUP_DATABASE_CONTAINER:?set FUDIAN_BACKUP_DATABASE_CONTAINER explicitly}"
backup_database_user="${FUDIAN_BACKUP_POSTGRES_USER:-fudian}"
backup_database_name="${FUDIAN_BACKUP_POSTGRES_DB:-fudian}"
backup_app_container="${FUDIAN_BACKUP_APP_CONTAINER:-}"
backup_artifact_volume="${FUDIAN_BACKUP_ARTIFACT_VOLUME:?set FUDIAN_BACKUP_ARTIFACT_VOLUME explicitly}"
backup_repository_volume="${FUDIAN_BACKUP_REPOSITORY_VOLUME:?set FUDIAN_BACKUP_REPOSITORY_VOLUME explicitly}"
backup_worktree_volume="${FUDIAN_BACKUP_WORKTREE_VOLUME:?set FUDIAN_BACKUP_WORKTREE_VOLUME explicitly}"
backup_runner_volume="${FUDIAN_BACKUP_RUNNER_VOLUME:?set FUDIAN_BACKUP_RUNNER_VOLUME explicitly}"
backup_archive_image="postgres:17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73"

validate_docker_name() {
  local label="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]]; then
    echo "$label 不是明确的 Docker 名称：$value" >&2
    exit 1
  fi
}

validate_docker_name "数据库容器" "$backup_database_container"
validate_docker_name "Artifact 卷" "$backup_artifact_volume"
validate_docker_name "repository 卷" "$backup_repository_volume"
validate_docker_name "worktree 卷" "$backup_worktree_volume"
validate_docker_name "Runner 卷" "$backup_runner_volume"
if [[ -n "$backup_app_container" ]]; then
  validate_docker_name "应用容器" "$backup_app_container"
  if [[ "$(docker inspect -f '{{.State.Running}}' "$backup_app_container" 2>/dev/null)" != true ]]; then
    echo "指定应用容器未运行：$backup_app_container" >&2
    exit 1
  fi
fi

if [[ "$(docker inspect -f '{{.State.Running}}' "$backup_database_container" 2>/dev/null)" != true ]]; then
  echo "指定 PostgreSQL 容器未运行：$backup_database_container" >&2
  exit 1
fi
for backup_volume in \
  "$backup_artifact_volume" "$backup_repository_volume" \
  "$backup_worktree_volume" "$backup_runner_volume"; do
  docker volume inspect "$backup_volume" >/dev/null
done

if [[ -n "$backup_app_container" ]]; then
  echo "备份前严格校验托管 Git 对象"
  docker exec "$backup_app_container" sh -ec '
    repository_root="${REPOSITORY_ROOT:-/data/repositories}"
    found=0
    for repository in "$repository_root"/projects/*.git "$repository_root"/maitu-code/*/repository.git; do
      [ -d "$repository" ] || continue
      found=1
      git --git-dir "$repository" fsck --strict
    done
    [ "$found" -eq 1 ] || [ -z "$(find "$repository_root" -mindepth 1 -print -quit)" ]
  '
fi

mkdir -p "$backup_parent"
backup_parent="$(cd "$backup_parent" && pwd)"
backup_partial="$backup_parent/.partial-$backup_timestamp-$$"
backup_final="$backup_parent/$backup_timestamp"
if [[ -e "$backup_partial" || -e "$backup_final" ]]; then
  echo "备份目标已经存在，拒绝覆盖：$backup_final" >&2
  exit 1
fi
mkdir "$backup_partial"

archive_volume() {
  local volume="$1"
  local archive_name="$2"
  local manifest_name="$3"
  docker run --rm \
    --network none \
    --read-only \
    --mount "type=volume,src=$volume,dst=/source,readonly" \
    --mount "type=bind,src=$backup_partial,dst=/backup" \
    "$backup_archive_image" \
    sh -ec "cd /source && find . -type f -print0 | sort -z | xargs -0 -r sha256sum >'/backup/$manifest_name' && tar -czf '/backup/$archive_name' ."
}

echo "备份 PostgreSQL：$backup_database_container"
docker exec "$backup_database_container" \
  pg_dump --username="$backup_database_user" --dbname="$backup_database_name" \
  --format=custom --no-owner --no-privileges >"$backup_partial/database.dump"

docker exec "$backup_database_container" \
  psql --username="$backup_database_user" --dbname="$backup_database_name" \
  --no-align --tuples-only --set=ON_ERROR_STOP=1 \
  --command="SELECT json_build_object(
    'schemaMigrationCount', (SELECT count(*) FROM schema_migrations),
    'schemaMigrations', (SELECT json_agg(filename ORDER BY filename) FROM schema_migrations),
    'projects', (SELECT count(*) FROM projects),
    'goalBranches', (SELECT count(*) FROM goal_branches),
    'goalSessions', (SELECT count(*) FROM goal_sessions),
    'artifacts', (SELECT count(*) FROM artifacts),
    'gitRepositories', (SELECT count(*) FROM project_git_repositories),
    'inputArtifacts', (SELECT count(*) FROM input_artifacts)
  );" >"$backup_partial/database-metadata.json"

echo "备份内容卷（全程只读挂载源卷）"
archive_volume "$backup_artifact_volume" artifacts.tar.gz artifacts.files.sha256
archive_volume "$backup_repository_volume" repositories.tar.gz repositories.files.sha256
archive_volume "$backup_worktree_volume" worktrees.tar.gz worktrees.files.sha256
archive_volume "$backup_runner_volume" runner-outputs.tar.gz runner-outputs.files.sha256

echo "备份当前可构建源码（排除 .env、secret、数据与 Git 凭据）"
tar -czf "$backup_partial/source.tar.gz" -C "$backup_repo_root" \
  Cargo.toml Cargo.lock Dockerfile compose.yaml compose.secure.yaml compose.secure-public.yaml compose.maitu.yaml \
  Makefile .env.example .env.secure.example README.md \
  src migrations assets deploy docs scripts recovery .github

(
  cd "$backup_repo_root"
  sha256sum migrations/*.sql
) >"$backup_partial/migration-directory.sha256"

backup_git_commit="$(git -C "$backup_repo_root" rev-parse HEAD 2>/dev/null || printf unknown)"
if [[ -z "$(git -C "$backup_repo_root" status --porcelain --untracked-files=normal 2>/dev/null)" ]]; then
  backup_git_dirty=false
else
  backup_git_dirty=true
fi
backup_database_id="$(docker inspect -f '{{.Id}}' "$backup_database_container")"
backup_database_image="$(docker inspect -f '{{.Image}}' "$backup_database_container")"
backup_source_revision="$backup_git_commit"
if [[ "$backup_git_dirty" == true ]]; then
  backup_source_revision="$backup_git_commit-dirty"
fi
if [[ -n "$backup_app_container" ]]; then
  backup_app_id="$(docker inspect -f '{{.Id}}' "$backup_app_container")"
  backup_app_image="$(docker inspect -f '{{.Image}}' "$backup_app_container")"
  backup_app_revision="$(docker inspect -f '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$backup_app_image" 2>/dev/null || printf '')"
  if [[ -z "$backup_app_revision" || "$backup_app_revision" == "<no value>" ]]; then
    backup_app_revision="not-recorded"
  fi
else
  backup_app_id="not-recorded"
  backup_app_image="not-recorded"
  backup_app_revision="not-recorded"
fi
if [[ "$backup_app_revision" == not-recorded ]]; then
  backup_source_matches="null"
elif [[ "${backup_app_revision%-dirty}" == "${backup_source_revision%-dirty}" ]]; then
  backup_source_matches="true"
else
  backup_source_matches="false"
fi
# 模型连接凭据按设计不进备份（只保存在 maitu_provider_config 卷里）。把它的位置写下来，
# 让每份备份自己说清边界，而不是等恢复时才发现连接是空的。
backup_credential_path="not-recorded"
backup_credential_volume="not-recorded"
if [[ -n "$backup_app_container" ]]; then
  backup_credential_path="$(docker inspect -f '{{range .Config.Env}}{{.}}{{"\n"}}{{end}}' "$backup_app_container" \
    | sed -n 's/^MAITU_CONFIG_ROOT=//p' | head -1)"
  [[ -n "$backup_credential_path" ]] || backup_credential_path="/data/maitu-config"
  backup_credential_volume="$(docker inspect -f '{{range .Mounts}}{{.Destination}}={{.Name}}{{"\n"}}{{end}}' "$backup_app_container" \
    | grep -F "$backup_credential_path=" | head -1 | cut -d= -f2- || true)"
  [[ -n "$backup_credential_volume" ]] || backup_credential_volume="not-recorded"
fi
printf '%s\n' \
  '{' \
  '  "schemaVersion": 3,' \
  "  \"createdAt\": \"$backup_timestamp\"," \
  "  \"sourceCommit\": \"$backup_git_commit\"," \
  "  \"sourceDirty\": $backup_git_dirty," \
  "  \"databaseContainerId\": \"$backup_database_id\"," \
  "  \"databaseImageId\": \"$backup_database_image\"," \
  "  \"appContainerId\": \"$backup_app_id\"," \
  "  \"appImageId\": \"$backup_app_image\"," \
  "  \"appImageRevision\": \"$backup_app_revision\"," \
  "  \"sourceMatchesRunningImage\": $backup_source_matches," \
  "  \"credentialStorePath\": \"$backup_credential_path\"," \
  "  \"credentialStoreVolume\": \"$backup_credential_volume\"," \
  '  "credentialStoreIncluded": false,' \
  '  "secretValuesIncluded": false,' \
  '  "storageClasses": ["artifacts", "repositories", "worktrees", "runner_outputs"]' \
  '}' >"$backup_partial/deployment-metadata.json"

(
  cd "$backup_partial"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%P\n' \
    | LC_ALL=C sort \
    | xargs -r sha256sum >SHA256SUMS
  sha256sum --check --strict SHA256SUMS >/dev/null
)
mv "$backup_partial" "$backup_final"
if [[ "$backup_source_matches" == false ]]; then
  echo "警告：归档源码是 $backup_source_revision，运行中的应用镜像标记为 $backup_app_revision。" >&2
  echo "      这次备份不能自证「归档源码就是产生该镜像的那份源码」，恢复后行为可能与当前实例不同。" >&2
  echo "      请先让工作树与构建该镜像的分支一致，再重新备份。" >&2
elif [[ "$backup_app_revision" == not-recorded ]]; then
  echo "提示：运行中的应用镜像没有源码修订标签，这次备份无法证明归档源码与运行镜像同源。" >&2
  echo "      用 scripts/start-local.ps1 或 compose.maitu.yaml 重新构建镜像后会带上该标签。" >&2
fi
if [[ "$backup_credential_path" != not-recorded ]]; then
  echo "提示：模型连接凭据（若已配置）存放在容器内 $backup_credential_path，按设计不在本次备份内。" >&2
  if [[ "$backup_credential_volume" != not-recorded ]]; then
    echo "      对应卷为 $backup_credential_volume；需要连同连接配置一起迁移或备份时，请单独复制该卷，并按凭据保管。" >&2
  else
    echo "      需要连同连接配置一起迁移或备份时，请单独复制承载该路径的卷，并按凭据保管。" >&2
  fi
fi

# 主机侧校验通过不等于能恢复：restore-v2.sh 用只读 bind 挂载读取这个目录，
# 而由容器创建在 Windows 挂载点上的目录可能主机可读、WSL/容器侧却不可见。
# 2026-10-10 实测到这样的目录：主机 sha256sum -c 全部 OK，容器只读 bind 挂载却报
# "mkdir ...: file exists" 且看不到任何文件，恢复因此直接失败。这里用恢复路径
# 同样的方式再自检一次，产出不可恢复就当场失败，不把成功写在报告里。
echo "用恢复路径的只读挂载自检备份目录"
backup_final_absolute="$(cd "$backup_final" && pwd)"
if ! backup_self_check="$(docker run --rm \
  --mount "type=bind,src=$backup_final_absolute,dst=/backup,readonly" \
  "$backup_archive_image" sh -c 'cd /backup && sha256sum -c SHA256SUMS >/dev/null 2>&1 && ls -1 | wc -l' 2>&1)"; then
  echo "备份目录无法被恢复路径读取，这份备份不能用于恢复：$backup_final" >&2
  echo "只读 bind 挂载自检输出：$backup_self_check" >&2
  exit 1
fi
host_file_count="$(find "$backup_final_absolute" -maxdepth 1 -type f 2>/dev/null | wc -l)"
container_file_count="${backup_self_check//[^0-9]/}"
if [[ "$host_file_count" -eq 0 || "$container_file_count" != "$host_file_count" ]]; then
  echo "备份目录在恢复路径下可见文件数与主机不一致：主机 $host_file_count，容器 $container_file_count" >&2
  exit 1
fi

echo "备份完成并通过校验：$backup_final"
