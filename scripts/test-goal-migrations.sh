#!/usr/bin/env bash
set -euo pipefail

migration_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
migration_container="fudian-goal-migration-test-$$"
migration_image="${POSTGRES_TEST_IMAGE:-postgres:17-alpine}"

cleanup_migration_container() {
  if [[ "$migration_container" == fudian-goal-migration-test-* ]]; then
    docker rm -fv "$migration_container" >/dev/null 2>&1 || true
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
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0003_tooling_core.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0004_input_artifacts.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0005_ideas_and_project_proposals.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0006_idea_sources.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0007_goal_domain_v2.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0008_context_memory.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0009_workspace_runner.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0010_signed_real_plugins.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0011_action_scheduler.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0012_review_integration.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0013_private_security_recovery.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0014_plugin_resources.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0015_maitu_file_workflows.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0016_maitu_project_execution.sql" >/dev/null 2>&1
# The compiled migration runner records applied files, but the SQL itself also remains safe to
# replay during recovery checks.
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0002_goal_branch_core.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0003_tooling_core.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0004_input_artifacts.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0005_ideas_and_project_proposals.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0006_idea_sources.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0007_goal_domain_v2.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0008_context_memory.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0009_workspace_runner.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0010_signed_real_plugins.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0011_action_scheduler.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0012_review_integration.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0013_private_security_recovery.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0014_plugin_resources.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0015_maitu_file_workflows.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/migrations/0016_maitu_project_execution.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/goal_core_constraints.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/idea_project_constraints.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/plugin_constraints.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/scheduler_constraints.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d fresh \
  < "$migration_repo_root/tests/sql/maitu_constraints.sql" >/dev/null

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
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0003_tooling_core.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0004_input_artifacts.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0005_ideas_and_project_proposals.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0006_idea_sources.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0007_goal_domain_v2.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0008_context_memory.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0009_workspace_runner.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0010_signed_real_plugins.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0011_action_scheduler.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0012_review_integration.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0013_private_security_recovery.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0014_plugin_resources.sql" >/dev/null
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0015_maitu_file_workflows.sql" >/dev/null 2>&1
docker exec -i "$migration_container" \
  psql -v ON_ERROR_STOP=1 -U fudian_test -d legacy \
  < "$migration_repo_root/migrations/0016_maitu_project_execution.sql" >/dev/null 2>&1

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
migration_tooling_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'plugin_packages', 'environment_manifests', 'session_environment_bindings',
     'tool_calls', 'tool_leases', 'plugin_publishers', 'plugin_installations',
     'plugin_install_requests', 'tool_execution_requests', 'plugin_package_resources',
     'plugin_resource_reads'
   )")"
migration_input_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'input_artifacts', 'input_artifact_chunks'
   )")"
migration_idea_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'ideas', 'idea_revisions', 'idea_links', 'project_proposals',
     'project_proposal_revisions', 'project_proposal_revision_ideas',
     'project_origins', 'project_origin_ideas', 'idea_command_receipts', 'idea_events',
     'idea_source_objects', 'idea_revision_sources'
   )")"
migration_workspace_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'project_git_repositories', 'goal_workspace_policies', 'goal_workspaces',
     'workspace_operations', 'workspace_snapshots', 'workspace_write_leases',
     'runner_jobs', 'runner_job_files'
   )")"
migration_scheduler_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'scheduler_workers', 'goal_action_runs', 'action_run_leases',
     'goal_action_events', 'goal_notifications', 'notification_outbox'
   )")"
migration_security_table_count="$(docker exec "$migration_container" \
  psql -U fudian_test -d legacy -Atc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema = 'public' AND table_name IN (
     'app_users', 'auth_recovery_codes', 'auth_sessions', 'auth_rate_limit_buckets',
     'security_audit_events', 'storage_reconciliation_runs',
     'storage_reconciliation_items'
   )")"

if [[ "$migration_counts_before" != "$migration_counts_after" ]]; then
  echo "legacy counts changed: $migration_counts_before -> $migration_counts_after" >&2
  exit 1
fi
if [[ "$migration_goal_table_count" != 31 ]]; then
  echo "expected 31 goal tables, found $migration_goal_table_count" >&2
  exit 1
fi
if [[ "$migration_tooling_table_count" != 11 ]]; then
  echo "expected 11 tooling tables, found $migration_tooling_table_count" >&2
  exit 1
fi
if [[ "$migration_input_table_count" != 2 ]]; then
  echo "expected 2 input tables, found $migration_input_table_count" >&2
  exit 1
fi
if [[ "$migration_idea_table_count" != 12 ]]; then
  echo "expected 12 idea/project-origin tables, found $migration_idea_table_count" >&2
  exit 1
fi
if [[ "$migration_workspace_table_count" != 8 ]]; then
  echo "expected 8 workspace/Runner tables, found $migration_workspace_table_count" >&2
  exit 1
fi
if [[ "$migration_scheduler_table_count" != 6 ]]; then
  echo "expected 6 scheduler/notification tables, found $migration_scheduler_table_count" >&2
  exit 1
fi
if [[ "$migration_security_table_count" != 7 ]]; then
  echo "expected 7 security/recovery tables, found $migration_security_table_count" >&2
  exit 1
fi

echo "goal migrations passed; legacy counts stayed $migration_counts_after"
