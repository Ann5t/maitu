#!/usr/bin/env bash
set -euo pipefail

maitu_test_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
maitu_test_prefix="maitu-workflow-test-$$"
maitu_test_db="$maitu_test_prefix-db"
maitu_test_app="$maitu_test_prefix-app"
maitu_test_network="$maitu_test_prefix-network"

cleanup_maitu_test() {
  local result="$?"
  [[ "$maitu_test_app" == maitu-workflow-test-*-app ]] \
    && docker rm -f "$maitu_test_app" >/dev/null 2>&1 || true
  [[ "$maitu_test_db" == maitu-workflow-test-*-db ]] \
    && docker rm -f "$maitu_test_db" >/dev/null 2>&1 || true
  [[ "$maitu_test_network" == maitu-workflow-test-*-network ]] \
    && docker network rm "$maitu_test_network" >/dev/null 2>&1 || true
  return "$result"
}
trap cleanup_maitu_test EXIT

docker network create "$maitu_test_network" >/dev/null
docker run -d --name "$maitu_test_db" --network "$maitu_test_network" \
  --network-alias maitu-test-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=maitu_test postgres:17-alpine >/dev/null
for maitu_test_attempt in $(seq 1 30); do
  if docker exec "$maitu_test_db" pg_isready -U fudian_test -d maitu_test >/dev/null 2>&1; then break; fi
  [[ "$maitu_test_attempt" == 30 ]] && docker logs "$maitu_test_db" && exit 1
  sleep 1
done

maitu_test_run_app() {
  docker run --rm --name "$maitu_test_app" --network "$maitu_test_network" \
    -e DATABASE_URL=postgres://fudian_test:fudian_test_only@maitu-test-db:5432/maitu_test \
    -e FUDIAN_SECURITY_MODE=disabled \
    -e ARTIFACT_ROOT=/tmp/maitu-test-artifacts \
    -e MAITU_CONFIG_ROOT=/tmp/maitu-test-settings \
    -e MAITU_CHECK_WORKER_TOKEN_FILE=/tmp/maitu-test-absent-check-worker \
    -e NO_PROXY=localhost,127.0.0.1,maitu-test-db \
    --mount "type=bind,src=$maitu_test_root,dst=/app" \
    --mount "type=volume,src=${MAITU_TEST_REGISTRY:-fudian_rust_cargo_registry},dst=/usr/local/cargo/registry" \
    --mount "type=volume,src=${MAITU_TEST_GIT:-fudian_rust_cargo_git},dst=/usr/local/cargo/git" \
    --mount "type=volume,src=${MAITU_TEST_TARGET:-fudian_rust_target},dst=/app/target" \
    "${MAITU_TEST_IMAGE:-fudian-nextgen-app:latest}" \
    cargo test --offline --locked --bin fudian "$@" -- --ignored --nocapture
}

maitu_test_run_app maitu::integration::parallel_files_retry_pinned_dependencies_and_process_recovery
maitu_test_run_app maitu::integration::rate_limited_connection_fails_over_bounded_and_records_usage_per_connection
maitu_test_run_app maitu::integration::queued_task_held_for_a_connection_is_not_claimed_by_it

echo "Maitu isolated workflow passed; this fixture does not verify paid DeepSeek access"
