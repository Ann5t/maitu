#!/usr/bin/env bash
# 备份当前 Maitu 栈：解析运行中的容器与四个内容卷，再交给 backup-v2.sh。
#
# 为什么不复用旧的 backup.sh：它按固定名字归档 fudian_nextgen_artifacts，而 Maitu
# 栈（compose.maitu.yaml）的卷名带 compose 项目前缀。名字对不上时旧脚本会用
# docker run 造出一个空卷并"成功"，产物是空归档。backup-v2.sh 会逐项
# docker volume inspect 自校验，所以这里只需把真实名字递过去。
#
# 名字来自运行中的应用容器的实际挂载和数据库容器的实际环境变量，而不是猜项目
# 前缀；任何一项解析不出来就直接失败，绝不产出看起来成功、实际为空的备份。
# 已经设置的 FUDIAN_BACKUP_* 变量优先，便于在别的项目名或容器名上复用。
set -euo pipefail

backup_entry_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
backup_entry_compose_file="${MAITU_COMPOSE_FILE:-$backup_entry_repo_root/compose.maitu.yaml}"

if [[ ! -f "$backup_entry_compose_file" ]]; then
  echo "找不到 compose 文件：$backup_entry_compose_file" >&2
  exit 1
fi

# Docker 守护进程不可用时，下面命令替换里的 docker compose ps 会被 set -e 直接终结，
# 且 stderr 已被压掉——用户将看到一个零输出的 exit 1。先显式探测，让失败自己说明原因。
if ! docker info >/dev/null 2>&1; then
  echo "Docker 守护进程不可用：请先启动 Docker（如 Docker Desktop），再运行备份。" >&2
  exit 1
fi

running_service_container() {
  local service="$1"
  docker compose -f "$backup_entry_compose_file" ps --format '{{.Name}}' "$service" 2>/dev/null | head -n 1
}

backup_entry_database_container="${FUDIAN_BACKUP_DATABASE_CONTAINER:-$(running_service_container postgres)}"
backup_entry_app_container="${FUDIAN_BACKUP_APP_CONTAINER:-$(running_service_container app)}"

backup_entry_missing=()
[[ -n "$backup_entry_database_container" ]] || backup_entry_missing+=("postgres")
[[ -n "$backup_entry_app_container" ]] || backup_entry_missing+=("app")
if [[ "${#backup_entry_missing[@]}" -gt 0 ]]; then
  echo "以下服务没有找到运行中的容器：${backup_entry_missing[*]}" >&2
  echo "请先用 scripts/start-local.ps1 启动本机 Maitu 栈：" >&2
  echo "  docker compose -f $(basename "$backup_entry_compose_file") up -d" >&2
  exit 1
fi

backup_entry_database_env="$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$backup_entry_database_container")"
environment_value() {
  sed -n "s/^$1=//p" <<<"$backup_entry_database_env" | head -n 1
}

backup_entry_database_user="${FUDIAN_BACKUP_POSTGRES_USER:-$(environment_value POSTGRES_USER)}"
backup_entry_database_name="${FUDIAN_BACKUP_POSTGRES_DB:-$(environment_value POSTGRES_DB)}"
if [[ -z "$backup_entry_database_user" || -z "$backup_entry_database_name" ]]; then
  echo "无法从 $backup_entry_database_container 的环境变量解析 PostgreSQL 用户与库名。" >&2
  echo "请显式设置 FUDIAN_BACKUP_POSTGRES_USER 与 FUDIAN_BACKUP_POSTGRES_DB。" >&2
  exit 1
fi

backup_entry_mounts="$(docker inspect -f '{{range .Mounts}}{{.Name}}{{"\t"}}{{.Destination}}{{"\n"}}{{end}}' "$backup_entry_app_container")"
volume_for_destination() {
  awk -v destination="$1" '$2 == destination { print $1; exit }' <<<"$backup_entry_mounts"
}

backup_entry_artifact_volume="${FUDIAN_BACKUP_ARTIFACT_VOLUME:-$(volume_for_destination /data/artifacts)}"
backup_entry_repository_volume="${FUDIAN_BACKUP_REPOSITORY_VOLUME:-$(volume_for_destination /data/repositories)}"
backup_entry_worktree_volume="${FUDIAN_BACKUP_WORKTREE_VOLUME:-$(volume_for_destination /data/worktrees)}"
backup_entry_runner_volume="${FUDIAN_BACKUP_RUNNER_VOLUME:-$(volume_for_destination /data/runner)}"

for backup_entry_pair in \
  "Artifact 卷:$backup_entry_artifact_volume" \
  "repository 卷:$backup_entry_repository_volume" \
  "worktree 卷:$backup_entry_worktree_volume" \
  "Runner 卷:$backup_entry_runner_volume"; do
  backup_entry_label="${backup_entry_pair%%:*}"
  backup_entry_value="${backup_entry_pair#*:}"
  if [[ -z "$backup_entry_value" ]]; then
    echo "无法从 $backup_entry_app_container 的挂载里解析出${backup_entry_label}。" >&2
    echo "请显式设置对应的 FUDIAN_BACKUP_*_VOLUME。" >&2
    exit 1
  fi
done

export FUDIAN_BACKUP_DATABASE_CONTAINER="$backup_entry_database_container"
export FUDIAN_BACKUP_POSTGRES_USER="$backup_entry_database_user"
export FUDIAN_BACKUP_POSTGRES_DB="$backup_entry_database_name"
export FUDIAN_BACKUP_APP_CONTAINER="$backup_entry_app_container"
export FUDIAN_BACKUP_ARTIFACT_VOLUME="$backup_entry_artifact_volume"
export FUDIAN_BACKUP_REPOSITORY_VOLUME="$backup_entry_repository_volume"
export FUDIAN_BACKUP_WORKTREE_VOLUME="$backup_entry_worktree_volume"
export FUDIAN_BACKUP_RUNNER_VOLUME="$backup_entry_runner_volume"

echo "Maitu 栈：数据库 $backup_entry_database_container（用户 $backup_entry_database_user，库 $backup_entry_database_name），应用 $backup_entry_app_container"
echo "内容卷：$backup_entry_artifact_volume、$backup_entry_repository_volume、$backup_entry_worktree_volume、$backup_entry_runner_volume"

exec "$backup_entry_repo_root/scripts/backup-v2.sh" "$@"