#!/usr/bin/env bash
set -euo pipefail

storage_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
storage_suffix="$$"
storage_network="fudian-storage-test-$storage_suffix"
storage_db="fudian-storage-db-$storage_suffix"
storage_app="fudian-storage-app-$storage_suffix"
storage_tmp="$(mktemp -d)"

# shellcheck source=scripts/docker-test-lib.sh
. "$storage_repo_root/scripts/docker-test-lib.sh"

cleanup_storage_test() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$storage_app" >/dev/null 2>&1; then
    docker logs "$storage_app" >&2 || true
  fi
  [[ "$storage_app" == fudian-storage-app-* ]] \
    && docker rm -f "$storage_app" >/dev/null 2>&1 || true
  [[ "$storage_db" == fudian-storage-db-* ]] \
    && docker rm -f "$storage_db" >/dev/null 2>&1 || true
  [[ "$storage_network" == fudian-storage-test-* ]] \
    && docker network rm "$storage_network" >/dev/null 2>&1 || true
  [[ "$storage_tmp" == /tmp/tmp.* && -d "$storage_tmp" ]] \
    && fudian_test_remove_bind_tree "$storage_tmp"
  return "$exit_status"
}
trap cleanup_storage_test EXIT

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

post_goal() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"action":sys.argv[1],"payload":json.loads(sys.argv[2])},ensure_ascii=False))' \
    "$action" "$payload" \
    | curl -fsS -H 'content-type: application/json' --data-binary @- \
      "$storage_base/api/v1/projects/$project_id/goal-commands"
}

run_maintenance() {
  docker run --rm --network "$storage_network" \
    -e DATABASE_URL=postgres://fudian_test:fudian_test_only@storage-db:5432/fudian_test \
    -e ARTIFACT_ROOT=/data/artifacts \
    -e REPOSITORY_ROOT=/data/repositories \
    -e WORKTREE_ROOT=/data/worktrees \
    -e RUNNER_OUTPUT_ROOT=/data/runner \
    -e FUDIAN_QUARANTINE_RETENTION_HOURS=0 \
    --mount "type=bind,src=$storage_repo_root,dst=/app" \
    --mount "type=bind,src=$storage_tmp/artifacts,dst=/data/artifacts" \
    --mount "type=bind,src=$storage_tmp/repositories,dst=/data/repositories" \
    --mount "type=bind,src=$storage_tmp/worktrees,dst=/data/worktrees" \
    --mount "type=bind,src=$storage_tmp/runner,dst=/data/runner" \
    --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
    --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
    --mount type=volume,src=fudian_rust_target,dst=/app/target \
    -w /app fudian-nextgen-app:latest \
    cargo run --quiet --locked --bin fudian-maintenance -- "$@"
}

mkdir -p "$storage_tmp/artifacts" "$storage_tmp/repositories" \
  "$storage_tmp/worktrees" "$storage_tmp/runner"
chmod 0777 "$storage_tmp/artifacts" "$storage_tmp/repositories" \
  "$storage_tmp/worktrees" "$storage_tmp/runner"
docker network create "$storage_network" >/dev/null
docker run -d --name "$storage_db" --network "$storage_network" \
  --network-alias storage-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for storage_attempt in $(seq 1 30); do
  if docker exec "$storage_db" pg_isready -U fudian_test -d fudian_test >/dev/null 2>&1; then break; fi
  [[ "$storage_attempt" == 30 ]] && docker logs "$storage_db" && exit 1
  sleep 1
done
docker run -d --name "$storage_app" --network "$storage_network" \
  -p 127.0.0.1::3000 --network-alias storage-app \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@storage-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/data/artifacts -e REPOSITORY_ROOT=/data/repositories \
  -e WORKTREE_ROOT=/data/worktrees -e RUNNER_OUTPUT_ROOT=/data/runner \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$storage_repo_root,dst=/app" \
  --mount "type=bind,src=$storage_tmp/artifacts,dst=/data/artifacts" \
  --mount "type=bind,src=$storage_tmp/repositories,dst=/data/repositories" \
  --mount "type=bind,src=$storage_tmp/worktrees,dst=/data/worktrees" \
  --mount "type=bind,src=$storage_tmp/runner,dst=/data/runner" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run --quiet --locked >/dev/null
storage_port="$(docker port "$storage_app" 3000/tcp | sed -n 's/.*://p')"
storage_base="http://127.0.0.1:$storage_port"
for storage_attempt in $(seq 1 60); do
  if curl -fsS "$storage_base/api/health" >/dev/null 2>&1; then break; fi
  [[ "$storage_attempt" == 60 ]] && docker logs "$storage_app" && exit 1
  sleep 1
done

storage_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"隔离存储对账验收"}' "$storage_base/api/projects")"
storage_project_id="$(json_field "$storage_project" id)"
storage_revision='{
  "whyNeeded":"验证存储引用与孤儿调和",
  "contract":{"desiredOutcome":"孤儿可发现、隔离并恢复","hardConstraints":["不直接删除"],
    "subjectivePreferences":[],"unknowns":[],"nonGoals":["不触碰生产"],
    "validationPlan":["扫描、quarantine、restore"],"judgmentTriggers":[],
    "stopConditions":["往返哈希一致"],"expectedContributions":["恢复证据"]},
  "expectedContributions":["恢复证据"],"explorationPlan":[],"contextInheritance":{},
  "toolRequirements":[],"capabilityPolicy":{"network":"denied","networkDestinations":[],
    "externalWrites":[],"accountReferences":[],"paidOperations":false,"deployment":false,
    "readScopes":["current_worktree"],"writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":128,"pids":64,
      "timeoutSeconds":120,"stdoutBytes":65536,"stderrBytes":65536}},
  "inferences":[],"revisionReason":null
}'
storage_proposal="$(post_goal "$storage_project_id" proposal.create \
  "{\"revision\":$storage_revision}")"
storage_proposal_id="$(json_field "$storage_proposal" result.proposalId)"
post_goal "$storage_project_id" proposal.submit \
  "{\"proposalId\":\"$storage_proposal_id\",\"expectedRevision\":1}" >/dev/null
storage_approval="$(post_goal "$storage_project_id" proposal.approve \
  "{\"proposalId\":\"$storage_proposal_id\",\"expectedRevision\":1,\"branchName\":\"存储调和目标\",\"assignment\":\"验证可逆调和\",\"agentIdentity\":\"storage-test-agent\"}")"
storage_branch_id="$(json_field "$storage_approval" result.goalBranchId)"
storage_session_id="$(json_field "$storage_approval" result.sessionId)"

docker exec --user 0:0 "$storage_app" sh -eu -c '
  mkdir -p /data/artifacts/records /data/artifacts/lost \
    /data/repositories/projects/orphan.git /data/worktrees/scratch \
    /data/runner/jobs/orphan
  printf "known artifact\n" >/data/artifacts/records/known.txt
  printf "orphan artifact\n" >/data/artifacts/lost/orphan.txt
  printf "orphan repository\n" >/data/repositories/projects/orphan.git/marker
  printf "orphan worktree\n" >/data/worktrees/scratch/marker
  printf "orphan runner output\n" >/data/runner/jobs/orphan/marker
'
storage_known_digest="$(sha256sum "$storage_tmp/artifacts/records/known.txt" | awk '{print $1}')"
storage_orphan_digest="$(sha256sum "$storage_tmp/artifacts/lost/orphan.txt" | awk '{print $1}')"
storage_artifact_id="$(new_uuid)"
storage_missing_id="$(new_uuid)"
storage_input_id="$(new_uuid)"
docker exec "$storage_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test \
  -c "INSERT INTO artifacts
    (id,project_id,title,kind,storage_path,media_type,sha256)
    VALUES ('$storage_artifact_id','$storage_project_id','known','text','records/known.txt','text/plain','$storage_known_digest'),
           ('$storage_missing_id','$storage_project_id','missing','text','records/missing.txt','text/plain','$(printf '2%.0s' $(seq 1 64))');
    INSERT INTO input_artifacts
    (id,project_id,goal_branch_id,session_id,client_request_id,status,original_filename,
     display_name,declared_size,storage_key)
    VALUES ('$storage_input_id','$storage_project_id','$storage_branch_id','$storage_session_id',
      '$(new_uuid)','staging','half.bin','half state',10,'inputs/staging/$storage_input_id/complete');" >/dev/null

storage_scan_1="$(run_maintenance scan)"
storage_scan_2="$(run_maintenance scan)"
python3 -c 'import json,sys
first=json.loads(sys.argv[1]); second=json.loads(sys.argv[2])
assert first["orphan"] == 4, first
assert first["missing"] == 2, first
assert first["referenced"] >= 3, first
assert first["digestMismatches"] == 0, first
assert {k:first[k] for k in ("referenced","orphan","missing","digestMismatches")} == \
       {k:second[k] for k in ("referenced","orphan","missing","digestMismatches")}' \
  "$storage_scan_1" "$storage_scan_2"
storage_scan_1_id="$(json_field "$storage_scan_1" runId)"
storage_scan_2_id="$(json_field "$storage_scan_2" runId)"
storage_item_diff="$(docker exec "$storage_db" psql -U fudian_test -d fudian_test -Atc \
  "WITH a AS (SELECT storage_class,relative_path,state FROM storage_reconciliation_items WHERE run_id='$storage_scan_1_id'),
        b AS (SELECT storage_class,relative_path,state FROM storage_reconciliation_items WHERE run_id='$storage_scan_2_id')
   SELECT (SELECT count(*) FROM (TABLE a EXCEPT TABLE b) x)+(SELECT count(*) FROM (TABLE b EXCEPT TABLE a) y);")"
[[ "$storage_item_diff" == 0 ]]

storage_quarantine="$(run_maintenance quarantine)"
storage_quarantine_id="$(json_field "$storage_quarantine" runId)"
[[ "$(json_field "$storage_quarantine" quarantined)" == 4 ]]
test ! -e "$storage_tmp/artifacts/lost"
test ! -e "$storage_tmp/repositories/projects/orphan.git"
test ! -e "$storage_tmp/worktrees/scratch"
test ! -e "$storage_tmp/runner/jobs/orphan"
test -f "$storage_tmp/artifacts/.fudian-quarantine/$storage_quarantine_id/lost/orphan.txt"
[[ "$(sha256sum "$storage_tmp/artifacts/.fudian-quarantine/$storage_quarantine_id/lost/orphan.txt" | awk '{print $1}')" == "$storage_orphan_digest" ]]

storage_restore="$(run_maintenance restore "$storage_quarantine_id")"
[[ "$(json_field "$storage_restore" restored)" == 4 ]]
test -f "$storage_tmp/artifacts/lost/orphan.txt"
test -f "$storage_tmp/repositories/projects/orphan.git/marker"
test -f "$storage_tmp/worktrees/scratch/marker"
test -f "$storage_tmp/runner/jobs/orphan/marker"
[[ "$(sha256sum "$storage_tmp/artifacts/lost/orphan.txt" | awk '{print $1}')" == "$storage_orphan_digest" ]]
test -f "$storage_tmp/artifacts/records/known.txt"
[[ "$(sha256sum "$storage_tmp/artifacts/records/known.txt" | awk '{print $1}')" == "$storage_known_digest" ]]

echo "storage reconciliation passed: stable scan, missing half-state, reversible quarantine and hash-identical restore"
