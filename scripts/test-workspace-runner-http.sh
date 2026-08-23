#!/usr/bin/env bash
set -euo pipefail

workspace_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workspace_suffix="$$"
workspace_network="fudian-workspace-test-$workspace_suffix"
workspace_db="fudian-workspace-db-$workspace_suffix"
workspace_app="fudian-workspace-app-$workspace_suffix"
workspace_tmp="$(mktemp -d)"

# shellcheck source=scripts/docker-test-lib.sh
. "$workspace_repo_root/scripts/docker-test-lib.sh"

cleanup_workspace_stack() {
  local exit_status="$?"
  if [[ "$exit_status" -ne 0 ]] && docker inspect "$workspace_app" >/dev/null 2>&1; then
    docker logs "$workspace_app" || true
  fi
  [[ "$workspace_app" == fudian-workspace-app-* ]] \
    && docker rm -f "$workspace_app" >/dev/null 2>&1 || true
  [[ "$workspace_db" == fudian-workspace-db-* ]] \
    && docker rm -f "$workspace_db" >/dev/null 2>&1 || true
  [[ "$workspace_network" == fudian-workspace-test-* ]] \
    && docker network rm "$workspace_network" >/dev/null 2>&1 || true
  if [[ "$workspace_tmp" == /tmp/tmp.* && -d "$workspace_tmp" ]]; then
    fudian_test_remove_bind_tree "$workspace_tmp"
  fi
  return "$exit_status"
}
trap cleanup_workspace_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys
value=json.loads(sys.argv[1])
for key in sys.argv[2].split("."):
    value=value[int(key)] if isinstance(value,list) else value[key]
print(str(value).lower() if isinstance(value,bool) else value)' "$1" "$2"
}

post_goal_for() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  local request_id="${4:-$(new_uuid)}"
  local body
  body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"action":sys.argv[2],"payload":json.loads(sys.argv[3])},ensure_ascii=False))' \
    "$request_id" "$action" "$payload")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$workspace_base/api/v1/projects/$project_id/goal-commands"
}

prepare_job() {
  local project_id="$1"
  local session_id="$2"
  local snapshot="$3"
  local relative_path="$4"
  local content="$5"
  local request_id="${6:-$(new_uuid)}"
  local payload
  payload="$(python3 -c 'import json,sys
print(json.dumps({
 "clientRequestId":sys.argv[1],
 "baseWorkspaceSnapshot":sys.argv[2],
 "allowedWrites":[sys.argv[3].split("/")[0]+"/**"],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write",sys.argv[3],sys.argv[4]],"environment":{}}
},ensure_ascii=False))' "$request_id" "$snapshot" "$relative_path" "$content")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$workspace_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs"
}

prepare_custom_job() {
  local project_id="$1"
  local session_id="$2"
  local snapshot="$3"
  local allowed_write="$4"
  local args_json="$5"
  local timeout_seconds="${6:-10}"
  local disk_mib="${7:-16}"
  local request_id="${8:-$(new_uuid)}"
  local payload
  payload="$(python3 -c 'import json,sys
print(json.dumps({
 "clientRequestId":sys.argv[1],
 "baseWorkspaceSnapshot":sys.argv[2],
 "allowedWrites":[sys.argv[3]],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":int(sys.argv[5]),"pids":32,"timeoutSeconds":int(sys.argv[4]),"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":json.loads(sys.argv[6]),"environment":{}}
},ensure_ascii=False))' "$request_id" "$snapshot" "$allowed_write" \
    "$timeout_seconds" "$disk_mib" "$args_json")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$workspace_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs"
}

prepare_delete_job() {
  local project_id="$1"
  local session_id="$2"
  local snapshot="$3"
  local relative_path="$4"
  local request_id="${5:-$(new_uuid)}"
  local payload
  payload="$(python3 -c 'import json,sys
print(json.dumps({
 "clientRequestId":sys.argv[1],
 "baseWorkspaceSnapshot":sys.argv[2],
 "allowedWrites":[sys.argv[3].split("/")[0]+"/**"],
 "deletePaths":[sys.argv[3]],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-sleep","0"],"environment":{}}
},ensure_ascii=False))' "$request_id" "$snapshot" "$relative_path")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$workspace_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs"
}

resume_session() {
  local project_id="$1"
  local session_id="$2"
  local resolution="$3"
  post_goal_for "$project_id" session.resume \
    "{\"sessionId\":\"$session_id\",\"resolution\":\"$resolution\"}" >/dev/null
}

fail_job() {
  local project_id="$1"
  local session_id="$2"
  local prepare_response="$3"
  local failure_kind="$4"
  local summary="$5"
  local job_id
  local token
  local payload
  job_id="$(json_field "$prepare_response" jobId)"
  token="$(json_field "$prepare_response" leaseToken)"
  payload="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"failureKind":sys.argv[2],"summary":sys.argv[3]},ensure_ascii=False))' \
    "$token" "$failure_kind" "$summary")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$workspace_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs/$job_id/fail"
}

execute_worker() {
  local prepare_response="$1"
  local worktree_key="$2"
  local label="$3"
  local spec_file="$workspace_tmp/$label-spec.json"
  local worktree_path="$workspace_tmp/worktrees/$worktree_key"
  local output_key
  local output_path
  output_key="$(json_field "$prepare_response" outputKey)"
  output_path="$workspace_tmp/runner/$output_key"
  python3 -c 'import json,sys
response=json.loads(sys.argv[1])
with open(sys.argv[2],"w",encoding="utf-8") as handle:
    json.dump(response["spec"],handle,separators=(",",":"),ensure_ascii=False)' \
    "$prepare_response" "$spec_file"
  fudian_test_open_runner_output "$workspace_app" "$output_key"
  docker run --rm \
    --network none \
    --read-only \
    --cap-drop ALL \
    --security-opt no-new-privileges:true \
    --pids-limit 32 \
    --memory 128m \
    --cpus 0.5 \
    --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
    --mount "type=bind,src=$worktree_path,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$output_path,dst=/workspace/output" \
    --mount "type=bind,src=$spec_file,dst=/workspace/result/spec.json,readonly" \
    fudian-nextgen-runner:latest execute /workspace/result/spec.json
}

finalize_job() {
  local project_id="$1"
  local session_id="$2"
  local prepare_response="$3"
  local runner_result="$4"
  local job_id
  local token
  local payload
  job_id="$(json_field "$prepare_response" jobId)"
  token="$(json_field "$prepare_response" leaseToken)"
  payload="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},ensure_ascii=False))' \
    "$token" "$runner_result")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$workspace_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs/$job_id/finalize"
}

mkdir -p "$workspace_tmp/artifacts" "$workspace_tmp/repositories" \
  "$workspace_tmp/worktrees" "$workspace_tmp/runner"
chmod 0777 "$workspace_tmp/artifacts" "$workspace_tmp/repositories" \
  "$workspace_tmp/worktrees" "$workspace_tmp/runner"

docker network create "$workspace_network" >/dev/null
docker run -d --name "$workspace_db" --network "$workspace_network" \
  --network-alias workspace-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for workspace_attempt in $(seq 1 30); do
  if docker exec "$workspace_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$workspace_attempt" == 30 ]] && docker logs "$workspace_db" && exit 1
  sleep 1
done

workspace_runtime_digest="$(docker run --rm --entrypoint /usr/local/bin/fudian-runner \
  fudian-nextgen-runner:latest digest)"

docker run -d --name "$workspace_app" --network "$workspace_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@workspace-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/data/artifacts \
  -e REPOSITORY_ROOT=/data/repositories \
  -e WORKTREE_ROOT=/data/worktrees \
  -e RUNNER_OUTPUT_ROOT=/data/runner \
  -e RUNNER_RUNTIME_DIGEST="$workspace_runtime_digest" \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$workspace_repo_root,dst=/app" \
  --mount "type=bind,src=$workspace_tmp/artifacts,dst=/data/artifacts" \
  --mount "type=bind,src=$workspace_tmp/repositories,dst=/data/repositories" \
  --mount "type=bind,src=$workspace_tmp/worktrees,dst=/data/worktrees" \
  --mount "type=bind,src=$workspace_tmp/runner,dst=/data/runner" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null

workspace_port="$(docker port "$workspace_app" 3000/tcp | sed -n 's/.*://p')"
workspace_base="http://127.0.0.1:$workspace_port"
for workspace_attempt in $(seq 1 60); do
  if curl -fsS "$workspace_base/api/health" >/dev/null 2>&1; then
    break
  fi
  if [[ "$workspace_attempt" == 60 ]]; then
    docker logs "$workspace_app"
    exit 1
  fi
  sleep 1
done

workspace_revision='{
  "whyNeeded":"验证真实 Git worktree 与隔离 Runner",
  "contract":{
    "desiredOutcome":"每条目标枝干具有可验证的独立工作现场",
    "hardConstraints":["单写 Lease","不暴露宿主秘密"],
    "subjectivePreferences":[],
    "unknowns":[],
    "nonGoals":["不执行远程 push"],
    "validationPlan":["运行真实 Git 与临时 Worker 流程"],
    "judgmentTriggers":[],
    "stopConditions":["路径、隔离、并发和资源测试通过"],
    "expectedContributions":["可追溯 Git commit"]
  },
  "expectedContributions":["可追溯 Git commit"],
  "explorationPlan":[],
  "contextInheritance":{},
  "toolRequirements":["fudian.runner"],
  "capabilityPolicy":{
    "network":"denied",
    "networkDestinations":[],
    "externalWrites":[],
    "accountReferences":[],
    "paidOperations":false,
    "deployment":false,
    "readScopes":["current_worktree","parent_snapshot"],
    "writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":256,"pids":64,"timeoutSeconds":300,"stdoutBytes":65536,"stderrBytes":65536}
  },
  "inferences":[],
  "revisionReason":null
}'

workspace_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证 worktree Runner"}' "$workspace_base/api/projects")"
workspace_project_id="$(json_field "$workspace_project_response" id)"
workspace_proposal_response="$(post_goal_for "$workspace_project_id" proposal.create \
  "{\"revision\":$workspace_revision}")"
workspace_proposal_id="$(json_field "$workspace_proposal_response" result.proposalId)"
post_goal_for "$workspace_project_id" proposal.submit \
  "{\"proposalId\":\"$workspace_proposal_id\",\"expectedRevision\":1}" >/dev/null
workspace_approval="$(post_goal_for "$workspace_project_id" proposal.approve \
  "{\"proposalId\":\"$workspace_proposal_id\",\"expectedRevision\":1,\"branchName\":\"Runner 根目标\",\"assignment\":\"验证真实执行\",\"agentIdentity\":\"runner-agent\"}")"
workspace_root_branch="$(json_field "$workspace_approval" result.goalBranchId)"
workspace_root_session="$(json_field "$workspace_approval" result.sessionId)"
workspace_root_key="$(json_field "$workspace_approval" result.workspace.workspace.worktreeKey)"
workspace_root_snapshot="$(json_field "$workspace_approval" result.workspace.workspace.workspaceSnapshot)"
[[ "$(json_field "$workspace_approval" result.workspace.matchesRecord)" == true ]]
[[ -f "$workspace_tmp/worktrees/$workspace_root_key/.git" ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_root_key" symbolic-ref --short HEAD)" == "goal/$workspace_root_branch" ]]

workspace_prepare="$(prepare_job "$workspace_project_id" "$workspace_root_session" \
  "$workspace_root_snapshot" "out/result.txt" "isolated-output")"
[[ "$(json_field "$workspace_prepare" replayed)" == false ]]
workspace_result="$(execute_worker "$workspace_prepare" "$workspace_root_key" root-write)"
python3 -c 'import json,sys
r=json.loads(sys.argv[1]); assert r["status"]=="succeeded"
i=r["isolation"]
assert i["networkIsolated"] and i["noNewPrivileges"] and i["rootReadOnly"]
assert i["inputReadOnly"] and i["outputWritable"]
assert i["dockerSocketAbsent"] and i["hostHomeAbsent"]
assert set(i["effectiveCapabilitiesHex"]) <= {"0"}' "$workspace_result"
workspace_finalize="$(finalize_job "$workspace_project_id" "$workspace_root_session" \
  "$workspace_prepare" "$workspace_result")"
[[ "$(json_field "$workspace_finalize" status)" == succeeded ]]
workspace_root_head="$(json_field "$workspace_finalize" headCommit)"
workspace_root_snapshot="$(json_field "$workspace_finalize" workspaceSnapshot)"
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_root_key" rev-parse HEAD)" == "$workspace_root_head" ]]
[[ "$(cat "$workspace_tmp/worktrees/$workspace_root_key/out/result.txt")" == isolated-output ]]
workspace_finalize_replay="$(finalize_job "$workspace_project_id" "$workspace_root_session" \
  "$workspace_prepare" "$workspace_result")"
[[ "$(json_field "$workspace_finalize_replay" replayed)" == true ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_root_key" rev-list --count HEAD)" == 2 ]]

# A successful no-op still creates an auditable operation/snapshot without inventing a commit.
workspace_noop_prepare="$(prepare_job "$workspace_project_id" "$workspace_root_session" \
  "$workspace_root_snapshot" "out/result.txt" "isolated-output")"
workspace_noop_result="$(execute_worker "$workspace_noop_prepare" "$workspace_root_key" root-noop)"
workspace_noop_finalize="$(finalize_job "$workspace_project_id" "$workspace_root_session" \
  "$workspace_noop_prepare" "$workspace_noop_result")"
[[ "$(json_field "$workspace_noop_finalize" status)" == succeeded ]]
[[ "$(json_field "$workspace_noop_finalize" headCommit)" == "$workspace_root_head" ]]
[[ "$(json_field "$workspace_noop_finalize" workspaceSnapshot)" == "$workspace_root_snapshot" ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_root_key" rev-list --count HEAD)" == 2 ]]

workspace_child_proposal_response="$(post_goal_for "$workspace_project_id" session.propose_child \
  "{\"parentSessionId\":\"$workspace_root_session\",\"revision\":$workspace_revision}")"
workspace_child_proposal="$(json_field "$workspace_child_proposal_response" result.proposalId)"
workspace_child_approval="$(post_goal_for "$workspace_project_id" proposal.approve \
  "{\"proposalId\":\"$workspace_child_proposal\",\"expectedRevision\":1,\"branchName\":\"Runner 子目标\",\"assignment\":\"验证独立子现场\",\"agentIdentity\":\"runner-child\"}")"
workspace_child_branch="$(json_field "$workspace_child_approval" result.goalBranchId)"
workspace_child_session="$(json_field "$workspace_child_approval" result.sessionId)"
workspace_child_key="$(json_field "$workspace_child_approval" result.workspace.workspace.worktreeKey)"
workspace_child_base="$(json_field "$workspace_child_approval" result.workspace.workspace.baseCommit)"
workspace_child_snapshot="$(json_field "$workspace_child_approval" result.workspace.workspace.workspaceSnapshot)"
[[ "$workspace_child_base" == "$workspace_root_head" ]]
[[ "$workspace_child_key" != "$workspace_root_key" ]]
[[ "$(cat "$workspace_tmp/worktrees/$workspace_child_key/out/result.txt")" == isolated-output ]]

workspace_child_prepare="$(prepare_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/child.txt" "child-only")"
workspace_child_result="$(execute_worker "$workspace_child_prepare" "$workspace_child_key" child-write)"
workspace_child_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_prepare" "$workspace_child_result")"
[[ "$(json_field "$workspace_child_finalize" status)" == succeeded ]]
[[ "$(cat "$workspace_tmp/worktrees/$workspace_child_key/out/child.txt")" == child-only ]]
[[ ! -e "$workspace_tmp/worktrees/$workspace_root_key/out/child.txt" ]]
workspace_child_head="$(json_field "$workspace_child_finalize" headCommit)"
workspace_child_snapshot="$(json_field "$workspace_child_finalize" workspaceSnapshot)"

# Deletion is an explicit, authorized, immutable part of the Job/Lease and becomes a real commit.
workspace_delete_prepare="$(prepare_delete_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/child.txt")"
[[ "$(json_field "$workspace_delete_prepare" spec.deletePaths.0)" == out/child.txt ]]
workspace_delete_result="$(execute_worker "$workspace_delete_prepare" "$workspace_child_key" child-delete)"
workspace_delete_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_delete_prepare" "$workspace_delete_result")"
[[ "$(json_field "$workspace_delete_finalize" status)" == succeeded ]]
[[ ! -e "$workspace_tmp/worktrees/$workspace_child_key/out/child.txt" ]]
workspace_delete_head="$(json_field "$workspace_delete_finalize" headCommit)"
[[ "$workspace_delete_head" != "$workspace_child_head" ]]
workspace_child_head="$workspace_delete_head"
workspace_child_snapshot="$(json_field "$workspace_delete_finalize" workspaceSnapshot)"

# A requested deletion must name a tracked baseline file; failure pauses safely without moving HEAD.
workspace_missing_delete_prepare="$(prepare_delete_job "$workspace_project_id" \
  "$workspace_child_session" "$workspace_child_snapshot" "out/missing.txt")"
workspace_missing_delete_result="$(execute_worker "$workspace_missing_delete_prepare" \
  "$workspace_child_key" missing-delete)"
workspace_missing_delete_job="$(json_field "$workspace_missing_delete_prepare" jobId)"
workspace_missing_delete_token="$(json_field "$workspace_missing_delete_prepare" leaseToken)"
workspace_missing_delete_body="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},ensure_ascii=False))' \
  "$workspace_missing_delete_token" "$workspace_missing_delete_result")"
workspace_missing_delete_status="$(curl -sS -o "$workspace_tmp/missing-delete.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_missing_delete_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_missing_delete_job/finalize")"
[[ "$workspace_missing_delete_status" == 409 ]]
[[ "$(json_field "$(<"$workspace_tmp/missing-delete.json")" code)" == candidate_commit_failed ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM runner_jobs WHERE id = '$workspace_missing_delete_job'")" == failed ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_workspaces WHERE goal_branch_id = '$workspace_child_branch'")" == ready ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_child_key" rev-parse HEAD)" == "$workspace_child_head" ]]
resume_session "$workspace_project_id" "$workspace_child_session" \
  "不存在的删除目标未进入 Git，已检查并继续"

workspace_unscoped_delete_payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":["docs/**"],"deletePaths":["out/result.txt"],
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-sleep","0"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_unscoped_delete_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_unscoped_delete_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs")"
[[ "$workspace_unscoped_delete_status" == 422 ]]

# The Broker rejects an ambiguous output+deletion for the same path before building a candidate.
workspace_delete_collision_payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":["out/**"],"deletePaths":["out/result.txt"],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write","out/result.txt","ambiguous"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_delete_collision_prepare="$(curl -fsS -H 'content-type: application/json' \
  -d "$workspace_delete_collision_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs")"
workspace_delete_collision_result="$(execute_worker "$workspace_delete_collision_prepare" \
  "$workspace_child_key" delete-collision)"
workspace_delete_collision_job="$(json_field "$workspace_delete_collision_prepare" jobId)"
workspace_delete_collision_token="$(json_field "$workspace_delete_collision_prepare" leaseToken)"
workspace_delete_collision_body="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},ensure_ascii=False))' \
  "$workspace_delete_collision_token" "$workspace_delete_collision_result")"
workspace_delete_collision_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_delete_collision_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_delete_collision_job/finalize")"
[[ "$workspace_delete_collision_status" == 422 ]]
fail_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_delete_collision_prepare" runner_failed \
  "输出与删除清单冲突已在候选构建前拒绝" >/dev/null
resume_session "$workspace_project_id" "$workspace_child_session" \
  "输出与删除冲突没有写回，继续安全测试"

# Path validation is applied before a Lease exists, including traversal and platform prefixes.
workspace_unsafe_path_payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":["../escape"],
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write","out/x","x"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_unsafe_path_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_unsafe_path_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs")"
[[ "$workspace_unsafe_path_status" == 422 ]]

workspace_reserved_path_payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":[".git/**"],
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write",".git/config","x"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_reserved_path_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_reserved_path_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs")"
[[ "$workspace_reserved_path_status" == 422 ]]

workspace_high_risk_payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":["out/**"],
 "capabilities":{"network":"public_read_only","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write","out/x","x"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_high_risk_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_high_risk_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs")"
[[ "$workspace_high_risk_status" == 403 ]]

# The Broker re-hashes the shared output layer; changing bytes after Worker exit is rejected.
workspace_integrity_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-write","out/integrity.txt","trusted"]')"
workspace_integrity_result="$(execute_worker "$workspace_integrity_prepare" "$workspace_child_key" integrity)"
workspace_integrity_output_key="$(json_field "$workspace_integrity_prepare" outputKey)"
fudian_test_write_managed_file "$workspace_app" \
  "/data/runner/$workspace_integrity_output_key/out/integrity.txt" tampered
workspace_integrity_job="$(json_field "$workspace_integrity_prepare" jobId)"
workspace_integrity_token="$(json_field "$workspace_integrity_prepare" leaseToken)"
workspace_integrity_body="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])}))' \
  "$workspace_integrity_token" "$workspace_integrity_result")"
workspace_integrity_reject="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_integrity_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_integrity_job/finalize")"
[[ "$workspace_integrity_reject" == 409 ]]
fudian_test_write_managed_file "$workspace_app" \
  "/data/runner/$workspace_integrity_output_key/out/integrity.txt" trusted
workspace_integrity_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_integrity_prepare" "$workspace_integrity_result")"
workspace_child_head="$(json_field "$workspace_integrity_finalize" headCommit)"
workspace_child_snapshot="$(json_field "$workspace_integrity_finalize" workspaceSnapshot)"

# Two concurrent acquire requests serialize on the workspace; exactly one gets the next fence.
workspace_concurrent_payload_1="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],"allowedWrites":["out/**"],
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write","out/race-a.txt","a"],"environment":{}}
}))' "$workspace_child_snapshot")"
workspace_concurrent_payload_2="$(python3 -c 'import json,sys,uuid
print(json.dumps({
 "clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],"allowedWrites":["out/**"],
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write","out/race-b.txt","b"],"environment":{}}
}))' "$workspace_child_snapshot")"
curl -sS -o "$workspace_tmp/race-1.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_concurrent_payload_1" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs" \
  > "$workspace_tmp/race-1.status" &
workspace_race_pid_1="$!"
curl -sS -o "$workspace_tmp/race-2.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_concurrent_payload_2" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs" \
  > "$workspace_tmp/race-2.status" &
workspace_race_pid_2="$!"
wait "$workspace_race_pid_1"
wait "$workspace_race_pid_2"
workspace_race_statuses="$(sort "$workspace_tmp/race-1.status" "$workspace_tmp/race-2.status" | tr '\n' ' ')"
[[ "$workspace_race_statuses" == "201 409 " ]]
if [[ "$(<"$workspace_tmp/race-1.status")" == 201 ]]; then
  workspace_race_winner="$(<"$workspace_tmp/race-1.json")"
else
  workspace_race_winner="$(<"$workspace_tmp/race-2.json")"
fi
workspace_race_failure="$(fail_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_race_winner" runner_failed "并发测试主动终止唯一获租约 Job")"
[[ "$(json_field "$workspace_race_failure" status)" == failed ]]
[[ "$(json_field "$workspace_race_failure" sessionStatus)" == exception_paused ]]
resume_session "$workspace_project_id" "$workspace_child_session" "已确认只有一个并发写者，继续安全测试"

# A token from the previous fence cannot control a newly acquired Lease.
workspace_new_fence_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-write","out/new-fence.txt","new"]')"
workspace_old_token="$(json_field "$workspace_race_winner" leaseToken)"
workspace_new_fence_job="$(json_field "$workspace_new_fence_prepare" jobId)"
workspace_old_token_payload="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"failureKind":"runner_failed","summary":"stale fence attempt"}))' \
  "$workspace_old_token")"
workspace_old_token_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_old_token_payload" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_new_fence_job/fail")"
[[ "$workspace_old_token_status" == 403 ]]
fail_job "$workspace_project_id" "$workspace_child_session" "$workspace_new_fence_prepare" \
  runner_failed "完成旧 fencing token 拒绝测试" >/dev/null
resume_session "$workspace_project_id" "$workspace_child_session" "旧 fencing token 已被拒绝"

# Symlinks never enter the manifest or Git worktree.
workspace_symlink_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-symlink","out/escape-link","/etc/passwd"]')"
workspace_symlink_result="$(execute_worker "$workspace_symlink_prepare" "$workspace_child_key" symlink)"
[[ "$(json_field "$workspace_symlink_result" status)" == policy_denied ]]
workspace_symlink_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_symlink_prepare" "$workspace_symlink_result")"
[[ "$(json_field "$workspace_symlink_finalize" status)" == policy_denied ]]
[[ "$(json_field "$workspace_symlink_finalize" headCommit)" == "$workspace_child_head" ]]
[[ ! -L "$workspace_tmp/worktrees/$workspace_child_key/out/escape-link" ]]
resume_session "$workspace_project_id" "$workspace_child_session" "符号链接输出已隔离拒绝"

# A forged successful attestation is rejected, while the original safe receipt remains usable.
workspace_attest_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-write","out/attested.txt","safe"]')"
workspace_attest_result="$(execute_worker "$workspace_attest_prepare" "$workspace_child_key" attestation)"
workspace_attest_job="$(json_field "$workspace_attest_prepare" jobId)"
workspace_attest_token="$(json_field "$workspace_attest_prepare" leaseToken)"
workspace_forged_body="$(python3 -c 'import json,sys
r=json.loads(sys.argv[2]); r["isolation"]["rootReadOnly"]=False
print(json.dumps({"leaseToken":sys.argv[1],"result":r}))' \
  "$workspace_attest_token" "$workspace_attest_result")"
workspace_forged_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_forged_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_attest_job/finalize")"
[[ "$workspace_forged_status" == 403 ]]
workspace_case_collision_body="$(python3 -c 'import json,sys
r=json.loads(sys.argv[2]); duplicate=dict(r["files"][0]); duplicate["path"]=duplicate["path"].upper()
r["files"].append(duplicate); r["files"].sort(key=lambda item:item["path"])
print(json.dumps({"leaseToken":sys.argv[1],"result":r}))' \
  "$workspace_attest_token" "$workspace_attest_result")"
workspace_case_collision_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_case_collision_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_attest_job/finalize")"
[[ "$workspace_case_collision_status" == 422 ]]
workspace_attest_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_attest_prepare" "$workspace_attest_result")"
workspace_child_head="$(json_field "$workspace_attest_finalize" headCommit)"
workspace_child_snapshot="$(json_field "$workspace_attest_finalize" workspaceSnapshot)"

# Wall-clock, disk and pids exhaustion return a pause without moving HEAD.
workspace_timeout_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-sleep","3"]' 1 16)"
workspace_timeout_result="$(execute_worker "$workspace_timeout_prepare" "$workspace_child_key" timeout)"
[[ "$(json_field "$workspace_timeout_result" status)" == timed_out ]]
workspace_timeout_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_timeout_prepare" "$workspace_timeout_result")"
[[ "$(json_field "$workspace_timeout_finalize" status)" == timed_out ]]
[[ "$(json_field "$workspace_timeout_finalize" headCommit)" == "$workspace_child_head" ]]
resume_session "$workspace_project_id" "$workspace_child_session" "超时没有写回，继续资源测试"

workspace_disk_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-disk","out/large.bin","2097152"]' 10 1)"
workspace_disk_result="$(execute_worker "$workspace_disk_prepare" "$workspace_child_key" disk)"
[[ "$(json_field "$workspace_disk_result" status)" == policy_denied ]]
workspace_disk_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_disk_prepare" "$workspace_disk_result")"
[[ "$(json_field "$workspace_disk_finalize" headCommit)" == "$workspace_child_head" ]]
resume_session "$workspace_project_id" "$workspace_child_session" "超量输出未写回"

workspace_pids_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-pids","100"]' 10 16)"
workspace_pids_result="$(execute_worker "$workspace_pids_prepare" "$workspace_child_key" pids)"
[[ "$(json_field "$workspace_pids_result" status)" == failed ]]
workspace_pids_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_pids_prepare" "$workspace_pids_result")"
[[ "$(json_field "$workspace_pids_finalize" headCommit)" == "$workspace_child_head" ]]
resume_session "$workspace_project_id" "$workspace_child_session" "进程数限制生效且未写回"

# If the cgroup kills a memory-hungry runtime before a receipt exists, the scheduler records it.
workspace_memory_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-memory","256"]' 10 16)"
set +e
workspace_memory_result="$(execute_worker "$workspace_memory_prepare" "$workspace_child_key" memory 2>/dev/null)"
workspace_memory_exit="$?"
set -e
if [[ "$workspace_memory_exit" -eq 0 && -n "$workspace_memory_result" ]]; then
  workspace_memory_finalize="$(finalize_job "$workspace_project_id" "$workspace_child_session" \
    "$workspace_memory_prepare" "$workspace_memory_result")"
  [[ "$(json_field "$workspace_memory_finalize" status)" == failed ]]
else
  workspace_memory_failure="$(fail_job "$workspace_project_id" "$workspace_child_session" \
    "$workspace_memory_prepare" resource_exhausted "Worker 被 128MiB cgroup 内存上限终止")"
  [[ "$(json_field "$workspace_memory_failure" status)" == failed ]]
fi
resume_session "$workspace_project_id" "$workspace_child_session" "内存耗尽已形成持久失败记录"

# An out-of-band write during an active Lease is detected immediately before publish and preserved.
workspace_dirty_prepare="$(prepare_custom_job "$workspace_project_id" "$workspace_child_session" \
  "$workspace_child_snapshot" "out/**" '["fixture-write","out/never.txt","never"]')"
workspace_dirty_result="$(execute_worker "$workspace_dirty_prepare" "$workspace_child_key" dirty-race)"
fudian_test_write_managed_file "$workspace_app" \
  "/data/worktrees/$workspace_child_key/external.txt" 'untracked external change'
workspace_dirty_job="$(json_field "$workspace_dirty_prepare" jobId)"
workspace_dirty_token="$(json_field "$workspace_dirty_prepare" leaseToken)"
workspace_dirty_finalize_body="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},ensure_ascii=False))' \
  "$workspace_dirty_token" "$workspace_dirty_result")"
workspace_dirty_status="$(curl -sS -o "$workspace_tmp/dirty-finalize.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_dirty_finalize_body" \
  "$workspace_base/api/v1/projects/$workspace_project_id/sessions/$workspace_child_session/runner-jobs/$workspace_dirty_job/finalize")"
[[ "$workspace_dirty_status" == 409 ]]
[[ "$(json_field "$(<"$workspace_tmp/dirty-finalize.json")" code)" == workspace_conflict ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_sessions WHERE id = '$workspace_child_session'")" == exception_paused ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_workspaces WHERE goal_branch_id = '$workspace_child_branch'")" == error ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_child_key" rev-parse HEAD)" == "$workspace_child_head" ]]
[[ "$(cat "$workspace_tmp/worktrees/$workspace_child_key/external.txt")" == 'untracked external change' ]]
[[ ! -e "$workspace_tmp/worktrees/$workspace_child_key/out/never.txt" ]]

# Provisioning a child from a drifted parent is rejected before any child worktree exists,
# and the newly approved child Session becomes a visible recoverable pause.
workspace_drift_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证父 worktree 漂移时安全暂停"}' "$workspace_base/api/projects")"
workspace_drift_project_id="$(json_field "$workspace_drift_project_response" id)"
workspace_drift_root_proposal_response="$(post_goal_for "$workspace_drift_project_id" proposal.create \
  "{\"revision\":$workspace_revision}")"
workspace_drift_root_proposal="$(json_field "$workspace_drift_root_proposal_response" result.proposalId)"
post_goal_for "$workspace_drift_project_id" proposal.submit \
  "{\"proposalId\":\"$workspace_drift_root_proposal\",\"expectedRevision\":1}" >/dev/null
workspace_drift_root_approval="$(post_goal_for "$workspace_drift_project_id" proposal.approve \
  "{\"proposalId\":\"$workspace_drift_root_proposal\",\"expectedRevision\":1,\"branchName\":\"漂移父目标\",\"assignment\":\"验证分枝前父安全点\",\"agentIdentity\":\"drift-parent\"}")"
workspace_drift_root_session="$(json_field "$workspace_drift_root_approval" result.sessionId)"
workspace_drift_root_key="$(json_field "$workspace_drift_root_approval" result.workspace.workspace.worktreeKey)"
workspace_drift_child_response="$(post_goal_for "$workspace_drift_project_id" session.propose_child \
  "{\"parentSessionId\":\"$workspace_drift_root_session\",\"revision\":$workspace_revision}")"
workspace_drift_child_proposal="$(json_field "$workspace_drift_child_response" result.proposalId)"
fudian_test_write_managed_file "$workspace_app" \
  "/data/worktrees/$workspace_drift_root_key/drift.txt" 'parent drift before approval'
workspace_drift_approval_id="$(new_uuid)"
workspace_drift_approval_body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"action":"proposal.approve","payload":{
 "proposalId":sys.argv[2],"expectedRevision":1,"branchName":"不应启动的子目标",
 "assignment":"必须停在父现场校验","agentIdentity":"drift-child"}},ensure_ascii=False))' \
  "$workspace_drift_approval_id" "$workspace_drift_child_proposal")"
workspace_drift_approval_status="$(curl -sS -o "$workspace_tmp/drift-approval.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_drift_approval_body" \
  "$workspace_base/api/v1/projects/$workspace_drift_project_id/goal-commands")"
[[ "$workspace_drift_approval_status" == 409 ]]
workspace_drift_child_branch="$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT approved_goal_branch_id FROM goal_branch_proposals WHERE id = '$workspace_drift_child_proposal'")"
workspace_drift_child_session="$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT head_session_id FROM goal_branches WHERE id = '$workspace_drift_child_branch'")"
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_sessions WHERE id = '$workspace_drift_child_session'")" == exception_paused ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_attention_items WHERE goal_branch_id = '$workspace_drift_child_branch' AND status = 'open'")" == 1 ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_workspaces WHERE goal_branch_id = '$workspace_drift_child_branch'")" == 0 ]]

# A branch ref that moves after Worker execution wins over the candidate; CAS never overwrites it.
workspace_cas_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证 Git compare-and-swap 竞态"}' "$workspace_base/api/projects")"
workspace_cas_project_id="$(json_field "$workspace_cas_project_response" id)"
workspace_cas_proposal_response="$(post_goal_for "$workspace_cas_project_id" proposal.create \
  "{\"revision\":$workspace_revision}")"
workspace_cas_proposal="$(json_field "$workspace_cas_proposal_response" result.proposalId)"
post_goal_for "$workspace_cas_project_id" proposal.submit \
  "{\"proposalId\":\"$workspace_cas_proposal\",\"expectedRevision\":1}" >/dev/null
workspace_cas_approval="$(post_goal_for "$workspace_cas_project_id" proposal.approve \
  "{\"proposalId\":\"$workspace_cas_proposal\",\"expectedRevision\":1,\"branchName\":\"CAS 根目标\",\"assignment\":\"拒绝覆盖并发 ref\",\"agentIdentity\":\"cas-agent\"}")"
workspace_cas_branch="$(json_field "$workspace_cas_approval" result.goalBranchId)"
workspace_cas_session="$(json_field "$workspace_cas_approval" result.sessionId)"
workspace_cas_key="$(json_field "$workspace_cas_approval" result.workspace.workspace.worktreeKey)"
workspace_cas_snapshot="$(json_field "$workspace_cas_approval" result.workspace.workspace.workspaceSnapshot)"
workspace_cas_prepare="$(prepare_job "$workspace_cas_project_id" "$workspace_cas_session" \
  "$workspace_cas_snapshot" "out/candidate.txt" "candidate")"
workspace_cas_result="$(execute_worker "$workspace_cas_prepare" "$workspace_cas_key" cas-candidate)"
fudian_test_write_managed_file "$workspace_app" \
  "/data/worktrees/$workspace_cas_key/external-commit.txt" 'external commit wins'
docker exec "$workspace_app" git -C "/data/worktrees/$workspace_cas_key" add -- external-commit.txt
docker exec "$workspace_app" git -C "/data/worktrees/$workspace_cas_key" \
  -c user.name='CAS Test' -c user.email='cas@fudian.invalid' -c commit.gpgSign=false \
  commit -m 'test: out-of-band CAS winner' >/dev/null
workspace_cas_external_head="$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_cas_key" rev-parse HEAD)"
workspace_cas_job="$(json_field "$workspace_cas_prepare" jobId)"
workspace_cas_token="$(json_field "$workspace_cas_prepare" leaseToken)"
workspace_cas_finalize_body="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},ensure_ascii=False))' \
  "$workspace_cas_token" "$workspace_cas_result")"
workspace_cas_finalize_status="$(curl -sS -o "$workspace_tmp/cas-finalize.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$workspace_cas_finalize_body" \
  "$workspace_base/api/v1/projects/$workspace_cas_project_id/sessions/$workspace_cas_session/runner-jobs/$workspace_cas_job/finalize")"
[[ "$workspace_cas_finalize_status" == 409 ]]
[[ "$(json_field "$(<"$workspace_tmp/cas-finalize.json")" code)" == workspace_conflict ]]
[[ "$(docker exec "$workspace_app" git -C "/data/worktrees/$workspace_cas_key" rev-parse HEAD)" == "$workspace_cas_external_head" ]]
[[ ! -e "$workspace_tmp/worktrees/$workspace_cas_key/out/candidate.txt" ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM runner_jobs WHERE id = '$workspace_cas_job'")" == workspace_conflict ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_sessions WHERE id = '$workspace_cas_session'")" == exception_paused ]]
[[ "$(docker exec "$workspace_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_workspaces WHERE goal_branch_id = '$workspace_cas_branch'")" == error ]]

docker exec -i "$workspace_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test <<SQL >/dev/null
DO \$\$
BEGIN
  IF (SELECT count(*) FROM project_git_repositories WHERE project_id = '$workspace_project_id') <> 1
     OR (SELECT count(*) FROM goal_workspaces WHERE project_id = '$workspace_project_id') <> 2
     OR (SELECT count(*) FROM workspace_snapshots WHERE project_id = '$workspace_project_id')
          <> (SELECT count(*) FROM goal_workspaces WHERE project_id = '$workspace_project_id')
             + (SELECT count(*) FROM runner_jobs WHERE project_id = '$workspace_project_id' AND status = 'succeeded')
     OR (SELECT count(*) FROM workspace_write_leases WHERE project_id = '$workspace_project_id' AND status = 'released')
          <> (SELECT count(*) FROM runner_jobs WHERE project_id = '$workspace_project_id' AND status = 'succeeded')
     OR (SELECT count(*) FROM goal_contributions WHERE project_id = '$workspace_project_id' AND runner_job_id IS NOT NULL)
          <> (SELECT count(*) FROM runner_jobs WHERE project_id = '$workspace_project_id' AND status = 'succeeded')
     OR EXISTS (
          SELECT 1 FROM goal_contributions c JOIN runner_jobs j ON j.id = c.runner_job_id
          WHERE c.project_id = '$workspace_project_id'
            AND (c.kind <> 'code_change' OR c.goal_branch_id <> j.goal_branch_id
              OR c.session_id <> j.session_id OR c.body NOT LIKE '%' || j.candidate_commit || '%'))
     OR (SELECT count(*) FROM runner_jobs WHERE project_id = '$workspace_project_id' AND status IN ('failed','timed_out','policy_denied')) < 6
     OR EXISTS (SELECT 1 FROM workspace_write_leases WHERE project_id = '$workspace_project_id' AND status = 'active') THEN
    RAISE EXCEPTION 'unexpected workspace Runner audit counts';
  END IF;
END;
\$\$;
SQL

echo "workspace Runner HTTP flow passed: real worktrees, single writer, isolation, escape and resource limits"
