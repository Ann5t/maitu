#!/usr/bin/env bash
set -euo pipefail

runtime_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime_suffix="$$"
runtime_network="fudian-runtime-test-$runtime_suffix"
runtime_db="fudian-runtime-db-$runtime_suffix"
runtime_app="fudian-runtime-app-$runtime_suffix"
runtime_image="${RUNTIME_TEST_IMAGE:-fudian-nextgen-runtime:test}"
runtime_tmp="$(mktemp -d)"

cleanup_runtime_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$runtime_app" >/dev/null 2>&1; then
    docker logs "$runtime_app" >&2 || true
  fi
  [[ "$runtime_app" == fudian-runtime-app-* ]] \
    && docker rm -f "$runtime_app" >/dev/null 2>&1 || true
  [[ "$runtime_db" == fudian-runtime-db-* ]] \
    && docker rm -f "$runtime_db" >/dev/null 2>&1 || true
  [[ "$runtime_network" == fudian-runtime-test-* ]] \
    && docker network rm "$runtime_network" >/dev/null 2>&1 || true
  [[ "$runtime_tmp" == /tmp/tmp.* ]] && rm -rf "$runtime_tmp"
  return "$exit_status"
}
trap cleanup_runtime_stack EXIT

docker build --target runtime --tag "$runtime_image" "$runtime_repo_root"
docker network create "$runtime_network" >/dev/null
docker run -d --name "$runtime_db" --network "$runtime_network" \
  --network-alias runtime-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for runtime_attempt in $(seq 1 30); do
  if docker exec "$runtime_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  if [[ "$runtime_attempt" == 30 ]]; then
    docker logs "$runtime_db"
    exit 1
  fi
  sleep 1
done

docker run -d --name "$runtime_app" --network "$runtime_network" \
  -p 127.0.0.1::3000 \
  --read-only \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,mode=1777 \
  --tmpfs /data/artifacts:rw,nosuid,nodev,noexec,uid=1000,gid=1000,mode=0700 \
  --tmpfs /data/repositories:rw,nosuid,nodev,noexec,uid=1000,gid=1000,mode=0700 \
  --tmpfs /data/worktrees:rw,nosuid,nodev,uid=1000,gid=1000,mode=0700 \
  --tmpfs /data/runner:rw,nosuid,nodev,noexec,uid=1000,gid=1000,mode=0700 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@runtime-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/data/artifacts \
  -e REPOSITORY_ROOT=/data/repositories \
  -e WORKTREE_ROOT=/data/worktrees \
  -e RUNNER_OUTPUT_ROOT=/data/runner \
  -e RUST_LOG=fudian=info \
  "$runtime_image" >/dev/null

runtime_port="$(docker port "$runtime_app" 3000/tcp | sed -n 's/.*://p')"
runtime_base="http://127.0.0.1:$runtime_port"
for runtime_attempt in $(seq 1 60); do
  if curl -fsS "$runtime_base/api/health" > "$runtime_tmp/health.json" 2>/dev/null; then
    break
  fi
  if [[ "$runtime_attempt" == 60 ]]; then
    docker logs "$runtime_app"
    exit 1
  fi
  sleep 1
done

python3 -c 'import json,sys
health=json.load(open(sys.argv[1], encoding="utf-8"))
assert health["ok"] is True
assert health["name"] == "fudian"' "$runtime_tmp/health.json"

[[ "$(docker exec "$runtime_app" id -u)" == 1000 ]]
[[ "$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$runtime_app")" == true ]]

curl -fsS "$runtime_base/" > "$runtime_tmp/index.html"
grep -q '脉图' "$runtime_tmp/index.html"
curl -fsS "$runtime_base/assets/app.css" > "$runtime_tmp/app.css"
grep -q -- '--acid' "$runtime_tmp/app.css"

runtime_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"隔离生产镜像健康验收"}' "$runtime_base/api/projects")"
runtime_project_id="$(python3 -c 'import json,sys,uuid
project=json.loads(sys.argv[1])
uuid.UUID(project["id"])
print(project["id"])' "$runtime_project")"
curl -fsS "$runtime_base/api/projects/$runtime_project_id" > "$runtime_tmp/project.json"
python3 -c 'import json,sys
snapshot=json.load(open(sys.argv[1], encoding="utf-8"))
assert snapshot["project"]["intent"] == "隔离生产镜像健康验收"' \
  "$runtime_tmp/project.json"

runtime_migrations="$(docker exec "$runtime_db" \
  psql -U fudian_test -d fudian_test -Atc 'SELECT count(*) FROM schema_migrations')"
runtime_projects="$(docker exec "$runtime_db" \
  psql -U fudian_test -d fudian_test -Atc 'SELECT count(*) FROM projects')"
[[ "$runtime_migrations" == 14 ]]
[[ "$runtime_projects" == 1 ]]

echo "production image passed: non-root, read-only rootfs, 14 migrations and isolated write"
