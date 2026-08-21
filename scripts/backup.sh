#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
backup_parent=${1:-"$project_root/backups"}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)

mkdir -p "$backup_parent"
backup_parent=$(CDPATH= cd -- "$backup_parent" && pwd)
backup_dir="$backup_parent/$timestamp"
mkdir -p "$backup_dir"

compose() {
  docker compose -f "$project_root/compose.yaml" "$@"
}

if ! compose ps --status running postgres --quiet | grep -q .; then
  echo "PostgreSQL 服务未运行；请先启动 compose 中的 postgres 服务。" >&2
  exit 1
fi

echo "备份 PostgreSQL..."
compose exec -T postgres sh -c \
  'pg_dump --username="$POSTGRES_USER" --dbname="$POSTGRES_DB" --format=custom' \
  >"$backup_dir/database.dump"

echo "备份文件产物卷..."
docker run --rm \
  --volume fudian_nextgen_artifacts:/source:ro \
  --volume "$backup_dir:/backup" \
  postgres:17-alpine \
  tar -czf /backup/artifacts.tar.gz -C /source .

echo "备份可构建源码..."
tar -czf "$backup_dir/source.tar.gz" \
  -C "$project_root" \
  Cargo.toml Cargo.lock Dockerfile compose.yaml Makefile .env.example \
  README.md src migrations assets docs scripts recovery

(
  cd "$backup_dir"
  sha256sum database.dump artifacts.tar.gz source.tar.gz >SHA256SUMS
)

echo "备份完成：$backup_dir"
