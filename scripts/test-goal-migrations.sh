#!/usr/bin/env bash
set -euo pipefail

migration_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
migration_container="fudian-goal-migration-test-$$"
migration_image="${POSTGRES_TEST_IMAGE:-postgres:17-alpine}"

cleanup_migration_container() {
  if [[ "$migration_container" == fudian-goal-migration-test-* ]]; then
    docker rm -f "$migration_container" >/dev/null 2>&1 || true
  fi
}
trap cleanup_migration_container EXIT

docker run -d --name "$migration_container" \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fresh \
  "$migration_image" >/dev/null

for migration_attempt in $(seq 1 30); do
  if docker exec "$migration_container" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fresh >/dev/null 2>&1; then
    break
  fi
  if [[ "$migration_attempt" == 30 ]]; then
    docker logs "$migration_container"
    exit 1
  fi
  sleep 1
done

docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0001_rust_baseline.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0002_goal_branch_core.sql" >/dev/null 2>&1
# The compiled migration runner records applied files, but the SQL itself also remains safe to
# replay during recovery checks.
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0002_goal_branch_core.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/goal_core_constraints.sql" >/dev/null

docker exec "$migration_container" createdb -U fudian_test legacy
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0001_rust_baseline.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/tests/sql/legacy_graph_fixture.sql" >/dev/null

migration_counts_before="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) || ':' ||
          (SELECT count(*) FROM project_branches) || ':' ||
          (SELECT count(*) FROM project_nodes)
   FROM projects")"

docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0002_goal_branch_core.sql" >/dev/null

migration_counts_after="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) || ':' ||
          (SELECT count(*) FROM project_branches) || ':' ||
          (SELECT count(*) FROM project_nodes)
   FROM projects")"
migration_goal_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name LIKE 'goal_%'")"

if [[ "$migration_counts_before" != "$migration_counts_after" ]]; then
  echo "legacy counts changed: $migration_counts_before -> $migration_counts_after" >&2
  exit 1
fi
if [[ "$migration_goal_table_count" != 14 ]]; then
  echo "expected 14 goal tables, found $migration_goal_table_count" >&2
  exit 1
fi

echo "goal migrations passed; legacy counts stayed $migration_counts_after"
