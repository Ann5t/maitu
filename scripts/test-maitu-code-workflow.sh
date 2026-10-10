#!/usr/bin/env bash
set -euo pipefail

code_test_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
code_test_prefix="maitu-code-test-$$"
code_test_network="$code_test_prefix-network"
code_test_db="$code_test_prefix-db"
code_test_worker="$code_test_prefix-worker"
code_test_app="$code_test_prefix-app"
code_test_worktrees="$code_test_prefix-worktrees"
code_test_records="$code_test_prefix-records"
code_test_auth="$code_test_prefix-auth"
code_test_image="${MAITU_CODE_TEST_RUNTIME:-maitu-code-test-runtime:local}"
code_test_controller="${MAITU_CODE_TEST_CONTROLLER:-maitu-code-test-app:local}"

cleanup_code_test() {
  local result="$?"
  if (( result != 0 )); then
    docker version --format 'Docker server: {{.Server.Version}} API {{.Server.APIVersion}}' >&2 || true
    docker logs "$code_test_worker" >&2 || true
  fi
  for container in "$code_test_app" "$code_test_worker" "$code_test_db"; do
    [[ "$container" == maitu-code-test-* ]] && docker rm -fv "$container" >/dev/null 2>&1 || true
  done
  while read -r container; do
    [[ "$container" =~ ^[a-f0-9]{12,64}$ ]] && docker rm -fv "$container" >/dev/null 2>&1 || true
  done < <(docker ps -aq --filter "label=maitu.check.volume=$code_test_worktrees")
  [[ "$code_test_network" == maitu-code-test-*-network ]] && docker network rm "$code_test_network" >/dev/null 2>&1 || true
  for volume in "$code_test_worktrees" "$code_test_records" "$code_test_auth"; do
    [[ "$volume" == maitu-code-test-* ]] && docker volume rm "$volume" >/dev/null 2>&1 || true
  done
  return "$result"
}
trap cleanup_code_test EXIT

if [[ "${MAITU_CODE_TEST_SKIP_BUILD:-0}" != 1 ]]; then
  docker build --target runtime -t "$code_test_controller" "$code_test_root"
  docker build --target code-runtime -t "$code_test_image" "$code_test_root"
fi
docker network create "$code_test_network" >/dev/null
for volume in "$code_test_worktrees" "$code_test_records" "$code_test_auth"; do docker volume create "$volume" >/dev/null; done
docker run --rm --network none \
  --mount "type=volume,src=$code_test_worktrees,dst=/data/worktrees" \
  --mount "type=volume,src=$code_test_records,dst=/data/runner" \
  --mount "type=volume,src=$code_test_auth,dst=/run/maitu-executor" \
  postgres:17-alpine sh -ec 'umask 077; od -An -tx1 -N32 /dev/urandom | tr -d " \n" >/run/maitu-executor/token; chown -R 1000:1000 /data/worktrees /data/runner /run/maitu-executor'
docker run -d --name "$code_test_db" --network "$code_test_network" --network-alias code-test-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only -e POSTGRES_DB=code_test postgres:17-alpine >/dev/null
for attempt in $(seq 1 30); do
  if docker exec "$code_test_db" pg_isready -U fudian_test -d code_test >/dev/null 2>&1; then break; fi
  [[ "$attempt" == 30 ]] && exit 1
  sleep 1
done
docker run -d --name "$code_test_worker" --network "$code_test_network" --network-alias code-test-worker \
  --mount type=bind,src=/var/run/docker.sock,dst=/var/run/docker.sock \
  --mount "type=volume,src=$code_test_records,dst=/data/runner" \
  --mount "type=volume,src=$code_test_auth,dst=/run/maitu-executor,readonly" \
  -e "MAITU_WORKTREE_VOLUME=$code_test_worktrees" -e "MAITU_CODE_IMAGE=$code_test_image" \
  --user 0:0 "$code_test_controller" maitu-check-worker >/dev/null
for attempt in $(seq 1 30); do
  if docker exec "$code_test_worker" fudian healthcheck 127.0.0.1:3001 >/dev/null 2>&1; then break; fi
  [[ "$attempt" == 30 ]] && exit 1
  sleep 1
done
docker run --rm --name "$code_test_app" --network "$code_test_network" \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@code-test-db:5432/code_test \
  -e FUDIAN_SECURITY_MODE=disabled -e ARTIFACT_ROOT=/tmp/code-test-artifacts \
  -e REPOSITORY_ROOT=/tmp/code-test-repositories -e WORKTREE_ROOT=/data/worktrees \
  -e MAITU_CHECK_WORKER_URL=http://code-test-worker:3001 \
  --mount "type=bind,src=$code_test_root,dst=/app" \
  --mount "type=volume,src=$code_test_worktrees,dst=/data/worktrees" \
  --mount "type=volume,src=$code_test_auth,dst=/run/maitu-executor,readonly" \
  --mount "type=volume,src=${MAITU_TEST_REGISTRY:-fudian_rust_cargo_registry},dst=/usr/local/cargo/registry" \
  --mount "type=volume,src=${MAITU_TEST_GIT:-fudian_rust_cargo_git},dst=/usr/local/cargo/git" \
  --mount "type=volume,src=${MAITU_TEST_TARGET:-fudian_rust_target},dst=/app/target" \
  -w /app "${MAITU_TEST_IMAGE:-fudian-nextgen-app:latest}" \
  cargo test --offline --locked --bin fudian maitu::code_integration::plan_to_parallel_code_checks_adoption_conflict_and_recovery -- --ignored --nocapture

echo "Maitu plan and code workflow passed: protocol fixture with real isolated Node checks"
