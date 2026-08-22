#!/usr/bin/env bash
set -euo pipefail

recovery_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
recovery_suffix="$$"
recovery_prefix="fudian-recovery-test-$recovery_suffix"
recovery_network="$recovery_prefix-network"
recovery_source_db="$recovery_prefix-source-db"
recovery_source_app="$recovery_prefix-source-app"
recovery_target_db="$recovery_prefix-target-db"
recovery_current_app="$recovery_prefix-current-app"
recovery_rollback_app="$recovery_prefix-rollback-app"
recovery_old_image="$recovery_prefix-bp08"
recovery_current_image="${FUDIAN_RECOVERY_CURRENT_IMAGE:-fudian-nextgen-runtime:bp09-test}"
recovery_postgres_image="postgres:17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73"
recovery_tmp="$(mktemp -d)"
recovery_database_password="bp09_recovery_${recovery_suffix}_database_secret"

recovery_source_artifacts="$recovery_prefix-source-artifacts"
recovery_source_repositories="$recovery_prefix-source-repositories"
recovery_source_worktrees="$recovery_prefix-source-worktrees"
recovery_source_runner="$recovery_prefix-source-runner"
recovery_target_artifacts="$recovery_prefix-target-artifacts"
recovery_target_repositories="$recovery_prefix-target-repositories"
recovery_target_worktrees="$recovery_prefix-target-worktrees"
recovery_target_runner="$recovery_prefix-target-runner"
recovery_volumes=(
  "$recovery_source_artifacts"
  "$recovery_source_repositories"
  "$recovery_source_worktrees"
  "$recovery_source_runner"
  "$recovery_target_artifacts"
  "$recovery_target_repositories"
  "$recovery_target_worktrees"
  "$recovery_target_runner"
)

cleanup_recovery_test() {
  local exit_status="$?"
  if (( exit_status != 0 )); then
    for recovery_log_container in \
      "$recovery_source_app" "$recovery_current_app" "$recovery_rollback_app"; do
      if docker inspect "$recovery_log_container" >/dev/null 2>&1; then
        docker logs "$recovery_log_container" >&2 || true
      fi
    done
  fi
  for recovery_container in \
    "$recovery_source_app" "$recovery_current_app" "$recovery_rollback_app" \
    "$recovery_source_db" "$recovery_target_db"; do
    if [[ "$recovery_container" == "$recovery_prefix"-* ]]; then
      docker rm -f "$recovery_container" >/dev/null 2>&1 || true
    fi
  done
  if [[ "$recovery_network" == "$recovery_prefix"-* ]]; then
    docker network rm "$recovery_network" >/dev/null 2>&1 || true
  fi
  for recovery_volume in "${recovery_volumes[@]}"; do
    if [[ "$recovery_volume" == "$recovery_prefix"-* ]]; then
      docker volume rm "$recovery_volume" >/dev/null 2>&1 || true
    fi
  done
  if [[ "$recovery_old_image" == "$recovery_prefix"-* ]]; then
    docker image rm "$recovery_old_image" >/dev/null 2>&1 || true
  fi
  if [[ "$recovery_tmp" == /tmp/tmp.* && -d "$recovery_tmp" ]]; then
    rm -rf -- "$recovery_tmp"
  fi
  return "$exit_status"
}
trap cleanup_recovery_test EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys
value=json.loads(sys.argv[1])
for key in sys.argv[2].split("."):
    value=value[key]
print(value)' "$1" "$2"
}

wait_for_database() {
  local container="$1"
  for recovery_attempt in $(seq 1 40); do
    if docker exec "$container" pg_isready -U fudian -d fudian >/dev/null 2>&1; then
      return 0
    fi
    if [[ "$recovery_attempt" == 40 ]]; then
      docker logs "$container" >&2
      return 1
    fi
    sleep 1
  done
}

wait_for_app() {
  local container="$1"
  local port
  port="$(docker port "$container" 3000/tcp | sed -n 's/.*://p')"
  for recovery_attempt in $(seq 1 80); do
    if curl -fsS "http://127.0.0.1:$port/api/health" >/dev/null 2>&1; then
      printf '%s' "$port"
      return 0
    fi
    if [[ "$recovery_attempt" == 80 ]]; then
      docker logs "$container" >&2
      return 1
    fi
    sleep 1
  done
}

post_goal() {
  local project_id="$1"
  local base="$2"
  local action="$3"
  local payload="$4"
  python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"action":sys.argv[1],
                  "payload":json.loads(sys.argv[2])},ensure_ascii=False))' \
    "$action" "$payload" \
    | curl -fsS -H 'content-type: application/json' --data-binary @- \
      "$base/api/v1/projects/$project_id/goal-commands"
}

create_volume() {
  local volume="$1"
  local restore_target="${2:-false}"
  if [[ "$restore_target" == true ]]; then
    docker volume create --label com.fudian.restore-target=true "$volume" >/dev/null
  else
    docker volume create "$volume" >/dev/null
  fi
}

chown_volume() {
  local volume="$1"
  docker run --rm --network none --user 0:0 \
    --mount "type=volume,src=$volume,dst=/data" \
    "$recovery_postgres_image" sh -ec 'chown -R 1000:1000 /data'
}

run_app() {
  local container="$1"
  local image="$2"
  local database_alias="$3"
  local artifact_volume="$4"
  local repository_volume="$5"
  local worktree_volume="$6"
  local runner_volume="$7"
  shift 7
  docker run -d --name "$container" --network "$recovery_network" \
    -p 127.0.0.1::3000 \
    -e "DATABASE_URL=postgres://fudian:$recovery_database_password@$database_alias:5432/fudian" \
    -e FUDIAN_SECURITY_MODE=disabled \
    -e FUDIAN_BIND=0.0.0.0:3000 \
    -e ARTIFACT_ROOT=/data/artifacts \
    -e REPOSITORY_ROOT=/data/repositories \
    -e WORKTREE_ROOT=/data/worktrees \
    -e RUNNER_OUTPUT_ROOT=/data/runner \
    -e RUST_LOG=fudian=info \
    --mount "type=volume,src=$artifact_volume,dst=/data/artifacts${1:-}" \
    --mount "type=volume,src=$repository_volume,dst=/data/repositories${1:-}" \
    --mount "type=volume,src=$worktree_volume,dst=/data/worktrees${1:-}" \
    --mount "type=volume,src=$runner_volume,dst=/data/runner${1:-}" \
    "$image"
}

docker image inspect "$recovery_current_image" >/dev/null
git -C "$recovery_repo_root" cat-file -e '0a73f58^{commit}'
mkdir -p "$recovery_tmp/rollback-context" "$recovery_tmp/backups" "$recovery_tmp/source-check"
git -C "$recovery_repo_root" archive --format=tar 0a73f58 \
  | tar -xf - -C "$recovery_tmp/rollback-context"
docker build --quiet --target runtime --tag "$recovery_old_image" \
  "$recovery_tmp/rollback-context" >/dev/null

docker network create "$recovery_network" >/dev/null
for recovery_volume in \
  "$recovery_source_artifacts" "$recovery_source_repositories" \
  "$recovery_source_worktrees" "$recovery_source_runner"; do
  create_volume "$recovery_volume"
  chown_volume "$recovery_volume"
done

docker run -d --name "$recovery_source_db" --network "$recovery_network" \
  --network-alias recovery-source-db \
  -e POSTGRES_USER=fudian -e "POSTGRES_PASSWORD=$recovery_database_password" \
  -e POSTGRES_DB=fudian "$recovery_postgres_image" >/dev/null
wait_for_database "$recovery_source_db"

run_app "$recovery_source_app" "$recovery_old_image" recovery-source-db \
  "$recovery_source_artifacts" "$recovery_source_repositories" \
  "$recovery_source_worktrees" "$recovery_source_runner" >/dev/null
recovery_source_port="$(wait_for_app "$recovery_source_app")"
recovery_source_base="http://127.0.0.1:$recovery_source_port"

recovery_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"备份、空目标恢复、迁移升级与应用回退验收"}' \
  "$recovery_source_base/api/projects")"
recovery_project_id="$(json_field "$recovery_project" id)"
recovery_revision='{
  "whyNeeded":"验证可恢复边界",
  "contract":{"desiredOutcome":"数据与 Git 经升级和回退仍可用",
    "hardConstraints":["仅隔离空目标"],"subjectivePreferences":[],"unknowns":[],
    "nonGoals":["不触碰生产"],"validationPlan":["备份、恢复、升级、回退"],
    "judgmentTriggers":[],"stopConditions":["哈希和计数一致"],
    "expectedContributions":["恢复演练证据"]},
  "expectedContributions":["恢复演练证据"],"explorationPlan":[],
  "contextInheritance":{},"toolRequirements":[],
  "capabilityPolicy":{"network":"denied","networkDestinations":[],
    "externalWrites":[],"accountReferences":[],"paidOperations":false,
    "deployment":false,"readScopes":["current_worktree"],"writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":128,
      "pids":64,"timeoutSeconds":120,"stdoutBytes":65536,"stderrBytes":65536}},
  "inferences":[],"revisionReason":null
}'
recovery_proposal="$(post_goal "$recovery_project_id" "$recovery_source_base" \
  proposal.create "{\"revision\":$recovery_revision}")"
recovery_proposal_id="$(json_field "$recovery_proposal" result.proposalId)"
post_goal "$recovery_project_id" "$recovery_source_base" proposal.submit \
  "{\"proposalId\":\"$recovery_proposal_id\",\"expectedRevision\":1}" >/dev/null
recovery_approval="$(post_goal "$recovery_project_id" "$recovery_source_base" \
  proposal.approve \
  "{\"proposalId\":\"$recovery_proposal_id\",\"expectedRevision\":1,
    \"branchName\":\"恢复演练目标\",\"assignment\":\"保存跨版本恢复证据\",
    \"agentIdentity\":\"recovery-test-agent\"}")"
recovery_branch_id="$(json_field "$recovery_approval" result.goalBranchId)"
recovery_repository_key="$(json_field "$recovery_approval" result.workspace.repository.storageKey)"

# BP08 only calculated the canonical empty-tree ID and did not persist that object. The
# reviewed one-object repair makes the historical backup internally valid; current code
# has a regression test and writes the object during initialization.
docker exec "$recovery_source_app" sh -ec '
  printf "" | git --git-dir "$1" hash-object -w -t tree --stdin >/dev/null
  git --git-dir "$1" fsck --strict
' sh "/data/repositories/$recovery_repository_key"

docker exec --user 1000:1000 "$recovery_source_app" sh -ec '
  mkdir -p /data/artifacts/recovery /data/runner/recovery
  printf "artifact survives recovery\n" >/data/artifacts/recovery/artifact.txt
  printf "runner output survives recovery\n" >/data/runner/recovery/output.txt
'
recovery_artifact_digest="$(docker exec "$recovery_source_app" \
  sha256sum /data/artifacts/recovery/artifact.txt | awk '{print $1}')"
recovery_artifact_id="$(new_uuid)"
docker exec "$recovery_source_db" psql -v ON_ERROR_STOP=1 -U fudian -d fudian \
  -c "INSERT INTO artifacts
    (id,project_id,title,kind,storage_path,media_type,sha256)
    VALUES ('$recovery_artifact_id','$recovery_project_id',
      'recovery marker','text','recovery/artifact.txt','text/plain','$recovery_artifact_digest');" \
  >/dev/null

recovery_source_migrations="$(docker exec "$recovery_source_db" \
  psql -U fudian -d fudian -Atc 'SELECT count(*) FROM schema_migrations;')"
[[ "$recovery_source_migrations" == 12 ]]
recovery_source_counts="$(docker exec "$recovery_source_db" psql -U fudian -d fudian -Atc \
  'SELECT concat((SELECT count(*) FROM projects), chr(58),
                 (SELECT count(*) FROM goal_branches), chr(58),
                 (SELECT count(*) FROM artifacts));')"
[[ "$recovery_source_counts" == 1:1:1 ]]

FUDIAN_BACKUP_DATABASE_CONTAINER="$recovery_source_db" \
FUDIAN_BACKUP_POSTGRES_USER=fudian \
FUDIAN_BACKUP_POSTGRES_DB=fudian \
FUDIAN_BACKUP_APP_CONTAINER="$recovery_source_app" \
FUDIAN_BACKUP_ARTIFACT_VOLUME="$recovery_source_artifacts" \
FUDIAN_BACKUP_REPOSITORY_VOLUME="$recovery_source_repositories" \
FUDIAN_BACKUP_WORKTREE_VOLUME="$recovery_source_worktrees" \
FUDIAN_BACKUP_RUNNER_VOLUME="$recovery_source_runner" \
  "$recovery_repo_root/scripts/backup-v2.sh" "$recovery_tmp/backups" >/dev/null
recovery_bundle="$(find "$recovery_tmp/backups" -mindepth 1 -maxdepth 1 -type d \
  ! -name '.partial-*' -print -quit)"
test -n "$recovery_bundle"
(cd "$recovery_bundle" && sha256sum --check --strict SHA256SUMS >/dev/null)
python3 -c 'import json,sys
deployment=json.load(open(sys.argv[1],encoding="utf-8"))
database=json.load(open(sys.argv[2],encoding="utf-8"))
assert deployment["schemaVersion"] == 2
assert deployment["secretValuesIncluded"] is False
assert database["schemaMigrationCount"] == 12, database
assert database["projects"] == 1 and database["goalBranches"] == 1
assert database["artifacts"] == 1' \
  "$recovery_bundle/deployment-metadata.json" "$recovery_bundle/database-metadata.json"
tar -xzf "$recovery_bundle/source.tar.gz" -C "$recovery_tmp/source-check"
if grep -R -a -F -- "$recovery_database_password" \
  "$recovery_tmp/source-check" "$recovery_bundle/deployment-metadata.json" >/dev/null; then
  echo "备份源码或部署元数据泄漏数据库口令" >&2
  exit 1
fi

for recovery_volume in \
  "$recovery_target_artifacts" "$recovery_target_repositories" \
  "$recovery_target_worktrees" "$recovery_target_runner"; do
  create_volume "$recovery_volume" true
done
docker run -d --name "$recovery_target_db" --network "$recovery_network" \
  --network-alias recovery-target-db --label com.fudian.restore-target=true \
  -e POSTGRES_USER=fudian -e "POSTGRES_PASSWORD=$recovery_database_password" \
  -e POSTGRES_DB=fudian "$recovery_postgres_image" >/dev/null
wait_for_database "$recovery_target_db"

FUDIAN_RESTORE_CONFIRM=EMPTY_LABELED_TARGETS \
FUDIAN_RESTORE_DATABASE_CONTAINER="$recovery_target_db" \
FUDIAN_RESTORE_POSTGRES_USER=fudian \
FUDIAN_RESTORE_POSTGRES_DB=fudian \
FUDIAN_RESTORE_ARTIFACT_VOLUME="$recovery_target_artifacts" \
FUDIAN_RESTORE_REPOSITORY_VOLUME="$recovery_target_repositories" \
FUDIAN_RESTORE_WORKTREE_VOLUME="$recovery_target_worktrees" \
FUDIAN_RESTORE_RUNNER_VOLUME="$recovery_target_runner" \
FUDIAN_RESTORE_APP_IMAGE="$recovery_current_image" \
  "$recovery_repo_root/scripts/restore-v2.sh" "$recovery_bundle" >/dev/null

recovery_restored_migrations="$(docker exec "$recovery_target_db" \
  psql -U fudian -d fudian -Atc 'SELECT count(*) FROM schema_migrations;')"
[[ "$recovery_restored_migrations" == 12 ]]

run_app "$recovery_current_app" "$recovery_current_image" recovery-target-db \
  "$recovery_target_artifacts" "$recovery_target_repositories" \
  "$recovery_target_worktrees" "$recovery_target_runner" >/dev/null
recovery_current_port="$(wait_for_app "$recovery_current_app")"
curl -fsS "http://127.0.0.1:$recovery_current_port/projects/$recovery_project_id" >/dev/null
recovery_upgraded_state="$(docker exec "$recovery_target_db" psql -U fudian -d fudian -Atc \
  'SELECT concat((SELECT count(*) FROM schema_migrations), chr(58),
                 (SELECT count(*) FROM projects), chr(58),
                 (SELECT count(*) FROM goal_branches), chr(58),
                 (SELECT count(*) FROM artifacts), chr(58),
                 (SELECT count(*) FROM app_users));')"
[[ "$recovery_upgraded_state" == 14:1:1:1:0 ]]
recovery_restored_digest="$(docker exec "$recovery_current_app" \
  sha256sum /data/artifacts/recovery/artifact.txt | awk '{print $1}')"
[[ "$recovery_restored_digest" == "$recovery_artifact_digest" ]]

docker rm -f "$recovery_current_app" >/dev/null
run_app "$recovery_rollback_app" "$recovery_old_image" recovery-target-db \
  "$recovery_target_artifacts" "$recovery_target_repositories" \
  "$recovery_target_worktrees" "$recovery_target_runner" ',readonly' >/dev/null
recovery_rollback_port="$(wait_for_app "$recovery_rollback_app")"
curl -fsS "http://127.0.0.1:$recovery_rollback_port/projects/$recovery_project_id" >/dev/null
recovery_rollback_state="$(docker exec "$recovery_target_db" psql -U fudian -d fudian -Atc \
  'SELECT concat((SELECT count(*) FROM schema_migrations), chr(58),
                 (SELECT count(*) FROM projects), chr(58),
                 (SELECT count(*) FROM goal_branches), chr(58),
                 (SELECT count(*) FROM artifacts));')"
[[ "$recovery_rollback_state" == 14:1:1:1 ]]

echo "backup recovery passed: BP08 backup, empty labeled restore, 12-to-14 upgrade, hash preservation and BP08 application rollback"
