#!/usr/bin/env bash
set -euo pipefail

review_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
review_suffix="$$"
review_network="fudian-review-integration-test-$review_suffix"
review_db="fudian-review-integration-db-$review_suffix"
review_app="fudian-review-integration-app-$review_suffix"
review_tmp="$(mktemp -d)"
review_bootstrap="bootstrap_review_integration_0123456789abcdef"

# shellcheck source=scripts/docker-test-lib.sh
. "$review_repo_root/scripts/docker-test-lib.sh"

cleanup_review_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$review_app" >/dev/null 2>&1; then
    docker logs "$review_app" >&2 || true
  fi
  [[ "$review_app" == fudian-review-integration-app-* ]] \
    && docker rm -fv "$review_app" >/dev/null 2>&1 || true
  [[ "$review_db" == fudian-review-integration-db-* ]] \
    && docker rm -fv "$review_db" >/dev/null 2>&1 || true
  [[ "$review_network" == fudian-review-integration-test-* ]] \
    && docker network rm "$review_network" >/dev/null 2>&1 || true
  [[ "$review_tmp" == /tmp/tmp.* && -d "$review_tmp" ]] \
    && fudian_test_remove_bind_tree "$review_tmp"
  return "$exit_status"
}
trap cleanup_review_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys
value=json.loads(sys.argv[1])
for key in sys.argv[2].split("."):
    value=value[int(key)] if isinstance(value,list) else value[key]
print("null" if value is None else str(value).lower() if isinstance(value,bool) else value)' \
    "$1" "$2"
}

post_goal_for() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  local body
  body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":str(__import__("uuid").uuid4()),
 "action":sys.argv[1],"payload":json.loads(sys.argv[2])},ensure_ascii=False))' \
    "$action" "$payload")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$review_base/api/v1/projects/$project_id/goal-commands"
}

register_worker() {
  local worker_id="$1"
  local worker_token="$2"
  local display_name="$3"
  local capability="$4"
  curl -fsS -H 'content-type: application/json' \
    -H "x-fudian-worker-bootstrap: $review_bootstrap" \
    -d "$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"workerId":sys.argv[1],
 "workerToken":sys.argv[2],"displayName":sys.argv[3],"capabilities":[sys.argv[4]]}))' \
      "$worker_id" "$worker_token" "$display_name" "$capability")" \
    "$review_base/api/v1/scheduler/workers" >/dev/null
}

claim_action() {
  local worker_id="$1"
  local worker_token="$2"
  local lease_token="$3"
  local claim_response
  for _ in $(seq 1 20); do
    claim_response="$(curl -fsS -H 'content-type: application/json' \
      -d "{\"workerId\":\"$worker_id\",\"workerToken\":\"$worker_token\",\"clientRequestId\":\"$(new_uuid)\",\"leaseToken\":\"$lease_token\",\"softTtlSeconds\":120,\"hardTtlSeconds\":300}" \
      "$review_base/api/v1/scheduler/claim")"
    if python3 -c 'import json,sys; raise SystemExit(json.loads(sys.argv[1]).get("action") is None)' \
      "$claim_response"; then
      printf '%s' "$claim_response"
      return 0
    fi
    sleep 0.25
  done
  printf '%s' "$claim_response"
  return 1
}

lease_credentials() {
  local claim="$1"
  local worker_id="$2"
  local worker_token="$3"
  local lease_token="$4"
  python3 -c 'import json,sys
c=json.loads(sys.argv[1]); print(json.dumps({"workerId":sys.argv[2],
 "workerToken":sys.argv[3],"leaseId":c["lease"]["id"],
 "leaseToken":sys.argv[4],"fencingToken":c["lease"]["fencingToken"]}))' \
    "$claim" "$worker_id" "$worker_token" "$lease_token"
}

review_report_for_claim() {
  local claim="$1"
  python3 -c 'import json,sys
p=json.loads(sys.argv[1])["action"]["payload"]
print(json.dumps({"schemaVersion":1,
 "candidateDigest":p["candidateDigest"],"contractVersionId":p["contractVersionId"],
 "observedHeadCommit":p["headCommit"],"observedTreeId":p["treeId"],
 "observedWorkspaceSnapshot":p["workspaceSnapshot"],
 "environmentFingerprint":p["environmentFingerprint"],"decision":"recommend_accept",
 "rationale":"独立只读复验确认冻结候选满足当前契约",
 "contractCheck":{"desiredOutcome":"passed","hardConstraints":"passed","validationPlan":"passed"},
 "counterexamples":[],"retestEvidence":["只读候选文件与 Git tree 一致"],
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
    "$claim"
}

prepare_runner_job() {
  local project_id="$1"
  local session_id="$2"
  local snapshot="$3"
  local relative_path="$4"
  local content="$5"
  curl -fsS -H 'content-type: application/json' \
    -d "$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"baseWorkspaceSnapshot":sys.argv[1],
 "allowedWrites":[sys.argv[2].split("/")[0]+"/**"],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write",sys.argv[2],sys.argv[3]],"environment":{}}}))' \
      "$snapshot" "$relative_path" "$content")" \
    "$review_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs"
}

execute_runner_job() {
  local prepared="$1"
  local worktree_key="$2"
  local label="$3"
  local spec_file="$review_tmp/$label-spec.json"
  local output_key
  output_key="$(json_field "$prepared" outputKey)"
  python3 -c 'import json,sys
with open(sys.argv[2],"w",encoding="utf-8") as handle:
 json.dump(json.loads(sys.argv[1])["spec"],handle,separators=(",",":"),ensure_ascii=False)' \
    "$prepared" "$spec_file"
  fudian_test_open_runner_output "$review_app" "$output_key"
  docker run --rm --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges:true --pids-limit 32 --memory 128m --cpus 0.5 \
    --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
    --mount "type=bind,src=$review_tmp/worktrees/$worktree_key,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$review_tmp/runner/$output_key,dst=/workspace/output" \
    --mount "type=bind,src=$spec_file,dst=/workspace/result/spec.json,readonly" \
    fudian-nextgen-runner:latest execute /workspace/result/spec.json
}

finalize_runner_job() {
  local project_id="$1"
  local session_id="$2"
  local prepared="$3"
  local result="$4"
  local job_id
  job_id="$(json_field "$prepared" jobId)"
  curl -fsS -H 'content-type: application/json' \
    -d "$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])}))' \
      "$(json_field "$prepared" leaseToken)" "$result")" \
    "$review_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs/$job_id/finalize"
}

run_workspace_write() {
  local project_id="$1"
  local session_id="$2"
  local worktree_key="$3"
  local snapshot="$4"
  local relative_path="$5"
  local content="$6"
  local label="$7"
  local prepared
  local result
  local finalized
  prepared="$(prepare_runner_job "$project_id" "$session_id" "$snapshot" "$relative_path" "$content")"
  result="$(execute_runner_job "$prepared" "$worktree_key" "$label")"
  finalized="$(finalize_runner_job "$project_id" "$session_id" "$prepared" "$result")"
  printf '%s' "$finalized"
}

review_candidate() {
  local expected_project="$1"
  local worker_id="$2"
  local worker_token="$3"
  local lease_token="$4"
  local claim
  local credentials
  local action_id
  local repository_key
  local worktree_key
  local expected_head
  local expected_tree
  local bad_body
  local bad_status
  local report
  claim="$(claim_action "$worker_id" "$worker_token" "$lease_token")"
  [[ "$(json_field "$claim" action.projectId)" == "$expected_project" ]]
  [[ "$(json_field "$claim" action.kind)" == review ]]
  action_id="$(json_field "$claim" action.id)"
  repository_key="$(json_field "$claim" action.payload.repositoryKey)"
  worktree_key="$(json_field "$claim" action.payload.worktreeKey)"
  expected_head="$(json_field "$claim" action.payload.headCommit)"
  expected_tree="$(json_field "$claim" action.payload.treeId)"
  credentials="$(lease_credentials "$claim" "$worker_id" "$worker_token" "$lease_token")"
  report="$(review_report_for_claim "$claim")"
  docker run --rm --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges:true --pids-limit 32 --memory 128m --cpus 0.5 \
    --user 1000:1000 --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
    -e REVIEW_EXPECTED_HEAD="$expected_head" -e REVIEW_EXPECTED_TREE="$expected_tree" \
    --mount "type=bind,src=$review_tmp/worktrees/$worktree_key,dst=/candidate,readonly" \
    --mount "type=bind,src=$review_tmp/repositories/$repository_key,dst=/data/repositories/$repository_key,readonly" \
    fudian-nextgen-app:latest bash -euo pipefail -c '
      test "$(git -c safe.directory=/candidate -C /candidate rev-parse HEAD)" = "$REVIEW_EXPECTED_HEAD"
      test "$(git -c safe.directory=/candidate -C /candidate rev-parse HEAD^{tree})" = "$REVIEW_EXPECTED_TREE"
      test -z "$(git -c safe.directory=/candidate -C /candidate status --porcelain=v1)"
      ! touch /candidate/reviewer-forbidden 2>/dev/null
      test ! -e /var/run/docker.sock
      test ! -e /root/.ssh
      test "$(awk "/CapEff/{print \$2}" /proc/self/status)" = 0000000000000000
    '
  bad_body="$(python3 -c 'import json,sys
c=json.loads(sys.argv[1]); r=json.loads(sys.argv[2]); r["candidateDigest"]="sha256:"+"0"*64
c["result"]=r; print(json.dumps(c))' "$credentials" "$report")"
  bad_status="$(curl -sS -o "$review_tmp/bad-review.json" -w '%{http_code}' \
    -H 'content-type: application/json' -d "$bad_body" \
    "$review_base/api/v1/scheduler/action-runs/$action_id/complete")"
  [[ "$bad_status" == 409 ]]
  [[ "$(json_field "$(<"$review_tmp/bad-review.json")" code)" == review_candidate_mismatch ]]
  curl -fsS -H 'content-type: application/json' \
    -d "$(python3 -c 'import json,sys
c=json.loads(sys.argv[1]); c["result"]=json.loads(sys.argv[2]); print(json.dumps(c))' \
      "$credentials" "$report")" \
    "$review_base/api/v1/scheduler/action-runs/$action_id/complete" >/dev/null
}

review_revision='{
  "whyNeeded":"验证冻结候选、独立审核和真实父枝干集成",
  "contract":{
    "desiredOutcome":"选中的子目标结果真实进入父工作现场",
    "hardConstraints":["父 Git 只能 CAS 更新","未接受结果不进入父上下文"],
    "subjectivePreferences":[],"unknowns":[],"nonGoals":["不推送远程仓库"],
    "validationPlan":["独立只读复验并检查父 worktree"],"judgmentTriggers":[],
    "stopConditions":["Git、文件、上下文和状态全部一致"],
    "expectedContributions":["绑定 Runner commit 的代码 Contribution"]
  },
  "expectedContributions":["绑定 Runner commit 的代码 Contribution"],
  "explorationPlan":[],"contextInheritance":{},"toolRequirements":["fudian.runner"],
  "capabilityPolicy":{"network":"denied","networkDestinations":[],"externalWrites":[],
    "accountReferences":[],"paidOperations":false,"deployment":false,
    "readScopes":["current_worktree","parent_snapshot"],"writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":256,"pids":64,
      "timeoutSeconds":300,"stdoutBytes":65536,"stderrBytes":65536}},
  "inferences":[],"revisionReason":null
}'

mkdir -p "$review_tmp/artifacts" "$review_tmp/repositories" \
  "$review_tmp/worktrees" "$review_tmp/runner"
chmod 0777 "$review_tmp/artifacts" "$review_tmp/repositories" \
  "$review_tmp/worktrees" "$review_tmp/runner"

docker network create "$review_network" >/dev/null
docker run -d --name "$review_db" --network "$review_network" --network-alias review-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for review_attempt in $(seq 1 30); do
  docker exec "$review_db" pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test \
    >/dev/null 2>&1 && break
  [[ "$review_attempt" == 30 ]] && docker logs "$review_db" && exit 1
  sleep 1
done

review_runner_digest="$(docker run --rm --entrypoint /usr/local/bin/fudian-runner \
  fudian-nextgen-runner:latest digest)"
docker run -d --name "$review_app" --network "$review_network" -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@review-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled \
  -e FUDIAN_BIND=0.0.0.0:3000 -e ARTIFACT_ROOT=/data/artifacts \
  -e REPOSITORY_ROOT=/data/repositories -e WORKTREE_ROOT=/data/worktrees \
  -e RUNNER_OUTPUT_ROOT=/data/runner -e RUNNER_RUNTIME_DIGEST="$review_runner_digest" \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$review_bootstrap" -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$review_repo_root,dst=/app" \
  --mount "type=bind,src=$review_tmp/artifacts,dst=/data/artifacts" \
  --mount "type=bind,src=$review_tmp/repositories,dst=/data/repositories" \
  --mount "type=bind,src=$review_tmp/worktrees,dst=/data/worktrees" \
  --mount "type=bind,src=$review_tmp/runner,dst=/data/runner" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run --locked >/dev/null

review_port="$(docker port "$review_app" 3000/tcp | sed -n 's/.*://p')"
review_base="http://127.0.0.1:$review_port"
for review_attempt in $(seq 1 60); do
  curl -fsS "$review_base/api/health" >/dev/null 2>&1 && break
  if [[ "$review_attempt" == 60 ]]; then docker logs "$review_app"; exit 1; fi
  sleep 1
done

review_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证真实审核与父枝干集成"}' "$review_base/api/projects")"
review_project_id="$(json_field "$review_project" id)"
review_root_proposal_response="$(post_goal_for "$review_project_id" proposal.create \
  "{\"revision\":$review_revision}")"
review_root_proposal="$(json_field "$review_root_proposal_response" result.proposalId)"
post_goal_for "$review_project_id" proposal.submit \
  "{\"proposalId\":\"$review_root_proposal\",\"expectedRevision\":1}" >/dev/null
review_root_approval="$(post_goal_for "$review_project_id" proposal.approve \
  "{\"proposalId\":\"$review_root_proposal\",\"expectedRevision\":1,\"branchName\":\"父目标\",\"assignment\":\"接收子目标并复验\",\"agentIdentity\":\"parent-agent\"}")"
review_root_branch="$(json_field "$review_root_approval" result.goalBranchId)"
review_root_session="$(json_field "$review_root_approval" result.sessionId)"
review_root_contract="$(json_field "$review_root_approval" result.contractVersionId)"
review_root_key="$(json_field "$review_root_approval" result.workspace.workspace.worktreeKey)"
review_root_snapshot="$(json_field "$review_root_approval" result.workspace.workspace.workspaceSnapshot)"
review_root_write="$(run_workspace_write "$review_project_id" "$review_root_session" \
  "$review_root_key" "$review_root_snapshot" "base/parent.txt" "parent-safe-point" root-base)"
review_root_snapshot="$(json_field "$review_root_write" workspaceSnapshot)"
review_parent_head_before="$(json_field "$review_root_write" headCommit)"

review_child_proposal_response="$(post_goal_for "$review_project_id" session.propose_child \
  "{\"parentSessionId\":\"$review_root_session\",\"revision\":$review_revision}")"
review_child_proposal="$(json_field "$review_child_proposal_response" result.proposalId)"
review_child_approval="$(post_goal_for "$review_project_id" proposal.approve \
  "{\"proposalId\":\"$review_child_proposal\",\"expectedRevision\":1,\"branchName\":\"子目标\",\"assignment\":\"形成两个可集成代码结果\",\"agentIdentity\":\"child-agent\"}")"
review_child_branch="$(json_field "$review_child_approval" result.goalBranchId)"
review_child_session="$(json_field "$review_child_approval" result.sessionId)"
review_child_key="$(json_field "$review_child_approval" result.workspace.workspace.worktreeKey)"
review_child_snapshot="$(json_field "$review_child_approval" result.workspace.workspace.workspaceSnapshot)"
review_child_write_a="$(run_workspace_write "$review_project_id" "$review_child_session" \
  "$review_child_key" "$review_child_snapshot" "feature/accepted.txt" "accepted-result" child-a)"
review_child_snapshot="$(json_field "$review_child_write_a" workspaceSnapshot)"
review_child_write_b="$(run_workspace_write "$review_project_id" "$review_child_session" \
  "$review_child_key" "$review_child_snapshot" "feature/also-accepted.txt" "second-result" child-b)"
review_child_snapshot="$(json_field "$review_child_write_b" workspaceSnapshot)"
review_child_head="$(json_field "$review_child_write_b" headCommit)"

review_graph="$(curl -fsS "$review_base/api/v1/projects/$review_project_id/goal-graph")"
review_child_contract="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(next(x for x in s["branches"] if x["id"]==branch)["currentContractVersionId"])' \
  "$review_graph" "$review_child_branch")"
review_child_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$review_graph" "$review_child_branch")"
[[ "$(python3 -c 'import json,sys; print(len(json.loads(sys.argv[1])))' "$review_child_contributions")" == 2 ]]

review_merge_payload="$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":json.loads(sys.argv[2]),
 "evidenceIds":[],"contractVersionId":sys.argv[3],"gitBaseCommit":"f"*40,
 "gitHeadCommit":"e"*40,"treeId":"d"*40,
 "workspaceSnapshot":"sha256:"+"c"*64,"gitDirty":True,
 "environmentFingerprint":"sha256:"+"b"*64,"testEvidence":["Runner 输出已提交"],
 "risks":[],"selfCheck":"已逐条检查目标、依赖和代码提交边界"}}))' \
    "$review_child_session" "$review_child_contributions" "$review_child_contract")"

# A moving workspace cannot be frozen.  The first rejection holds a real write
# Lease; the second observes an out-of-band dirty file.  Both are cleared at an
# explicit safe boundary before the authoritative server-side binding proceeds.
review_busy_prepared="$(prepare_runner_job "$review_project_id" "$review_child_session" \
  "$review_child_snapshot" "busy/never-applied.txt" "not-applied")"
review_busy_body="$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"action":"merge.propose",
 "payload":json.loads(sys.argv[1])}))' "$review_merge_payload")"
review_busy_status="$(curl -sS -o "$review_tmp/busy-merge.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$review_busy_body" \
  "$review_base/api/v1/projects/$review_project_id/goal-commands")"
[[ "$review_busy_status" == 409 ]]
[[ "$(json_field "$(<"$review_tmp/busy-merge.json")" code)" == workspace_busy ]]
curl -fsS -H 'content-type: application/json' \
  -d "$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"failureKind":"cancelled",
 "summary":"专项验证后显式释放未执行写 Lease"}))' \
    "$(json_field "$review_busy_prepared" leaseToken)")" \
  "$review_base/api/v1/projects/$review_project_id/sessions/$review_child_session/runner-jobs/$(json_field "$review_busy_prepared" jobId)/fail" \
  >/dev/null
post_goal_for "$review_project_id" session.resume \
  "{\"sessionId\":\"$review_child_session\",\"resolution\":\"已确认专项 Runner 未执行并释放 Lease，继续冻结检查\"}" \
  >/dev/null
docker exec "$review_app" touch "/data/worktrees/$review_child_key/reviewer-drift.tmp"
review_dirty_body="$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"action":"merge.propose",
 "payload":json.loads(sys.argv[1])}))' "$review_merge_payload")"
review_dirty_status="$(curl -sS -o "$review_tmp/dirty-merge.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$review_dirty_body" \
  "$review_base/api/v1/projects/$review_project_id/goal-commands")"
[[ "$review_dirty_status" == 409 ]]
[[ "$(json_field "$(<"$review_tmp/dirty-merge.json")" code)" == workspace_record_drifted ]]
docker exec "$review_app" git -C "/data/worktrees/$review_child_key" clean -f \
  -- reviewer-drift.tmp >/dev/null

review_merge="$(post_goal_for "$review_project_id" merge.propose "$review_merge_payload")"
review_gate="$(json_field "$review_merge" result.reviewGateId)"
review_digest="$(json_field "$review_merge" result.candidateDigest)"
review_bound_graph="$(curl -fsS "$review_base/api/v1/projects/$review_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); gate=sys.argv[2]; head=sys.argv[3]; snapshot=sys.argv[4]
g=next(x for x in s["reviewGates"] if x["id"]==gate)
assert g["gitHeadCommit"]==head and g["workspaceSnapshot"]==snapshot and not g["gitDirty"]
assert g["candidateSnapshot"]["gitHeadCommit"]==head
assert g["candidateSnapshot"]["workspaceSnapshot"]==snapshot
assert g["candidateSnapshot"]["environmentFingerprint"] is None' \
  "$review_bound_graph" "$review_gate" "$review_child_head" "$review_child_snapshot"
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_workspaces WHERE goal_branch_id = '$review_child_branch'")" == frozen ]]
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$review_root_key" rev-parse HEAD)" \
  == "$review_parent_head_before" ]]

review_worker_id="$(new_uuid)"
review_worker_token="review_worker_token_0123456789abcdef"
register_worker "$review_worker_id" "$review_worker_token" "independent-reviewer" \
  review.goal_candidate.v1
review_candidate "$review_project_id" "$review_worker_id" "$review_worker_token" \
  review_lease_token_0123456789abcdef
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_review_gates WHERE id = '$review_gate'")" == pending_human_review ]]

review_human="$(post_goal_for "$review_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"accept",
 "rationale":"独立审核与冻结证据清楚，授权真实父枝干集成",
 "selectedContributionIds":json.loads(sys.argv[2])}))' \
    "$review_gate" "$review_child_contributions")")"
review_integration="$(json_field "$review_human" result.integrationId)"
[[ "$(json_field "$review_human" result.gitIntegrationStatus)" == pending ]]
[[ "$(json_field "$review_human" result.goalBranchStatus)" == review_pending ]]
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_sessions WHERE id = '$review_child_session'")" == awaiting_merge_review ]]
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$review_root_key" rev-parse HEAD)" \
  == "$review_parent_head_before" ]]
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_context_entries e JOIN goal_context_snapshot_entries m ON m.entry_id=e.id JOIN goal_sessions s ON s.context_snapshot_id=m.snapshot_id WHERE s.id='$review_root_session' AND e.origin_goal_branch_id='$review_child_branch'")" == 0 ]]

integration_worker_id="$(new_uuid)"
integration_worker_token="integration_worker_token_0123456789abcdef"
register_worker "$integration_worker_id" "$integration_worker_token" "integration-validator" \
  integration.goal_branch.v1
integration_lease_token="integration_lease_token_0123456789abcdef"
integration_claim="$(claim_action "$integration_worker_id" "$integration_worker_token" \
  "$integration_lease_token")"
[[ "$(json_field "$integration_claim" action.subjectId)" == "$review_integration" ]]
integration_action="$(json_field "$integration_claim" action.id)"
integration_credentials="$(lease_credentials "$integration_claim" "$integration_worker_id" \
  "$integration_worker_token" "$integration_lease_token")"
integration_prepare="$(curl -fsS -H 'content-type: application/json' \
  -d "$integration_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$integration_action/integrations/$review_integration/prepare")"
integration_key="$(json_field "$integration_prepare" preparationKey)"
integration_candidate="$(json_field "$integration_prepare" candidateCommit)"
integration_tree="$(json_field "$integration_prepare" candidateTreeId)"
integration_snapshot="$(json_field "$integration_prepare" candidateWorkspaceSnapshot)"
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$review_root_key" rev-parse HEAD)" \
  == "$review_parent_head_before" ]]

docker run --rm --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges:true --pids-limit 32 --memory 128m --cpus 0.5 \
  --user 1000:1000 --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
  --mount "type=bind,src=$review_tmp/runner/$integration_key,dst=/candidate,readonly" \
  --mount "type=bind,src=$review_tmp/repositories,dst=/data/repositories,readonly" \
  fudian-nextgen-app:latest bash -euo pipefail -c '
    test "$(git -c safe.directory=/candidate -C /candidate rev-parse HEAD)" = "'"$integration_candidate"'"
    test "$(git -c safe.directory=/candidate -C /candidate rev-parse HEAD^{tree})" = "'"$integration_tree"'"
    test "$(< /candidate/feature/accepted.txt)" = accepted-result
    test "$(< /candidate/feature/also-accepted.txt)" = second-result
    ! touch /candidate/forbidden
    test ! -e /var/run/docker.sock
    test ! -e /root/.ssh
    test "$(awk "/CapEff/{print \$2}" /proc/self/status)" = 0000000000000000
  '

integration_validation="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"candidateCommit":sys.argv[1],"candidateTreeId":sys.argv[2],
 "candidateWorkspaceSnapshot":sys.argv[3],"status":"passed",
 "checks":["Git tree 与冻结选择一致","父目标文件回归通过"],
 "contractCheck":{"desiredOutcome":"passed","hardConstraints":"passed","validationPlan":"passed"},
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
  "$integration_candidate" "$integration_tree" "$integration_snapshot")"
integration_finalize_body="$(python3 -c 'import json,sys
body=json.loads(sys.argv[1]); body["validation"]=json.loads(sys.argv[2]); print(json.dumps(body))' \
  "$integration_credentials" "$integration_validation")"
integration_finalize="$(curl -fsS -H 'content-type: application/json' \
  -d "$integration_finalize_body" \
  "$review_base/api/v1/scheduler/action-runs/$integration_action/integrations/$review_integration/finalize")"
[[ "$(json_field "$integration_finalize" status)" == applied ]]
review_parent_head_after="$(json_field "$integration_finalize" targetHeadCommit)"
[[ "$review_parent_head_after" == "$integration_candidate" ]]
[[ "$(<"$review_tmp/worktrees/$review_root_key/feature/accepted.txt")" == accepted-result ]]
[[ "$(<"$review_tmp/worktrees/$review_root_key/feature/also-accepted.txt")" == second-result ]]

review_final_graph="$(curl -fsS "$review_base/api/v1/projects/$review_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); child=sys.argv[2]; parent=sys.argv[3]; child_session=sys.argv[4]; parent_session=sys.argv[5]; integration=sys.argv[6]
assert next(x for x in s["branches"] if x["id"]==child)["status"]=="integrated"
assert next(x for x in s["branches"] if x["id"]==parent)["status"]=="active"
assert next(x for x in s["sessions"] if x["id"]==child_session)["status"]=="accepted"
assert next(x for x in s["sessions"] if x["id"]==parent_session)["status"]=="running"
i=next(x for x in s["integrations"] if x["id"]==integration)
assert i["gitIntegrationStatus"]=="applied" and i["candidateCommit"]
assert s["project"]["state"]=="active"' \
  "$review_final_graph" "$review_child_branch" "$review_root_branch" \
  "$review_child_session" "$review_root_session" "$review_integration"
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_context_entries e JOIN goal_context_snapshot_entries m ON m.entry_id=e.id JOIN goal_sessions s ON s.context_snapshot_id=m.snapshot_id WHERE s.id='$review_root_session' AND e.origin_goal_branch_id='$review_child_branch' AND e.source_kind='contribution'")" == 2 ]]
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_integrations WHERE project_id='$review_project_id' AND git_integration_status='not_attempted'")" == 0 ]]

# The applied child Integration is itself an authoritative explanation for the
# parent's new HEAD.  The parent can now add its own conclusion, enter the same
# independent review protocol, and only the human can complete the root goal.
review_root_contribution_response="$(post_goal_for "$review_project_id" session.add_contribution \
  "{\"sessionId\":\"$review_root_session\",\"kind\":\"evidence\",\"title\":\"父目标整合结论\",\"body\":\"已核对真实子枝干集成后的父工作现场\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}")"
review_root_contribution="$(json_field "$review_root_contribution_response" result.contributionId)"
review_graph="$(curl -fsS "$review_base/api/v1/projects/$review_project_id/goal-graph")"
review_root_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$review_graph" "$review_root_branch")"
[[ "$(python3 -c 'import json,sys; print(len(json.loads(sys.argv[1])))' \
  "$review_root_contributions")" == 2 ]]
review_root_merge="$(post_goal_for "$review_project_id" merge.propose \
  "$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":json.loads(sys.argv[2]),
 "evidenceIds":[],"contractVersionId":sys.argv[3],"gitBaseCommit":None,"gitHeadCommit":None,
 "gitDirty":False,"environmentFingerprint":None,"testEvidence":["父工作现场整合复验通过"],
 "risks":[],"selfCheck":"已确认子目标通过不曾自动完成父目标，并复验父契约"}}))' \
    "$review_root_session" "$review_root_contributions" "$review_root_contract")")"
review_root_gate="$(json_field "$review_root_merge" result.reviewGateId)"
review_candidate "$review_project_id" "$review_worker_id" "$review_worker_token" \
  root_review_lease_token_0123456789abcdef
review_root_human="$(post_goal_for "$review_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"accept",
 "rationale":"独立审核后确认根目标完成","selectedContributionIds":json.loads(sys.argv[2])}))' \
    "$review_root_gate" "$review_root_contributions")")"
[[ "$(json_field "$review_root_human" result.integrationId)" == null ]]
review_root_final="$(curl -fsS "$review_base/api/v1/projects/$review_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); root=sys.argv[2]; session=sys.argv[3]
assert s["project"]["state"]=="completed"
assert next(x for x in s["branches"] if x["id"]==root)["status"]=="completed"
assert next(x for x in s["sessions"] if x["id"]==session)["status"]=="accepted"
assert len(s["integrations"])==1' \
  "$review_root_final" "$review_root_branch" "$review_root_session"

# A second project proves that partial acceptance is a commit-level selection,
# not a whole-branch merge that accidentally carries later child history.
partial_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证部分接受只回流选中的代码提交"}' "$review_base/api/projects")"
partial_project_id="$(json_field "$partial_project" id)"
partial_root_proposal_response="$(post_goal_for "$partial_project_id" proposal.create \
  "{\"revision\":$review_revision}")"
partial_root_proposal="$(json_field "$partial_root_proposal_response" result.proposalId)"
post_goal_for "$partial_project_id" proposal.submit \
  "{\"proposalId\":\"$partial_root_proposal\",\"expectedRevision\":1}" >/dev/null
partial_root_approval="$(post_goal_for "$partial_project_id" proposal.approve \
  "{\"proposalId\":\"$partial_root_proposal\",\"expectedRevision\":1,\"branchName\":\"部分接受父目标\",\"assignment\":\"只接收选中结果\",\"agentIdentity\":\"partial-parent-agent\"}")"
partial_root_branch="$(json_field "$partial_root_approval" result.goalBranchId)"
partial_root_session="$(json_field "$partial_root_approval" result.sessionId)"
partial_root_key="$(json_field "$partial_root_approval" result.workspace.workspace.worktreeKey)"
partial_child_proposal_response="$(post_goal_for "$partial_project_id" session.propose_child \
  "{\"parentSessionId\":\"$partial_root_session\",\"revision\":$review_revision}")"
partial_child_proposal="$(json_field "$partial_child_proposal_response" result.proposalId)"
partial_child_approval="$(post_goal_for "$partial_project_id" proposal.approve \
  "{\"proposalId\":\"$partial_child_proposal\",\"expectedRevision\":1,\"branchName\":\"部分接受子目标\",\"assignment\":\"形成可独立选择的两个提交\",\"agentIdentity\":\"partial-child-agent\"}")"
partial_child_branch="$(json_field "$partial_child_approval" result.goalBranchId)"
partial_child_session="$(json_field "$partial_child_approval" result.sessionId)"
partial_child_contract="$(json_field "$partial_child_approval" result.contractVersionId)"
partial_child_key="$(json_field "$partial_child_approval" result.workspace.workspace.worktreeKey)"
partial_child_snapshot="$(json_field "$partial_child_approval" result.workspace.workspace.workspaceSnapshot)"
partial_write_a="$(run_workspace_write "$partial_project_id" "$partial_child_session" \
  "$partial_child_key" "$partial_child_snapshot" "chosen/keep.txt" "keep-this" partial-a)"
partial_child_snapshot="$(json_field "$partial_write_a" workspaceSnapshot)"
partial_write_b="$(run_workspace_write "$partial_project_id" "$partial_child_session" \
  "$partial_child_key" "$partial_child_snapshot" "discard/drop.txt" "do-not-integrate" partial-b)"
partial_graph="$(curl -fsS "$review_base/api/v1/projects/$partial_project_id/goal-graph")"
partial_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$partial_graph" "$partial_child_branch")"
partial_contribution_a="$(json_field "$partial_contributions" 0)"
partial_contribution_b="$(json_field "$partial_contributions" 1)"
partial_merge="$(post_goal_for "$partial_project_id" merge.propose \
  "$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":[sys.argv[2],sys.argv[3]],
 "evidenceIds":[],"contractVersionId":sys.argv[4],"gitBaseCommit":None,"gitHeadCommit":None,
 "gitDirty":False,"environmentFingerprint":None,"testEvidence":["两个提交边界已核对"],
 "risks":["只接受第一个提交"],"selfCheck":"完整候选包含全部代码，用户可选择真子集"}}))' \
    "$partial_child_session" "$partial_contribution_a" "$partial_contribution_b" \
    "$partial_child_contract")")"
partial_gate="$(json_field "$partial_merge" result.reviewGateId)"

# Worker identity is scheduler-authenticated.  A worker whose registered name
# equals the authoring Agent cannot turn its own candidate into an independent
# opinion; after its safe transient failure the old fencing token stays dead.
same_agent_worker_id="$(new_uuid)"
same_agent_worker_token="same_agent_worker_token_0123456789abcdef"
register_worker "$same_agent_worker_id" "$same_agent_worker_token" \
  partial-child-agent review.goal_candidate.v1
same_agent_lease_token="same_agent_lease_token_0123456789abcdef"
same_agent_claim="$(claim_action "$same_agent_worker_id" "$same_agent_worker_token" \
  "$same_agent_lease_token")"
[[ "$(json_field "$same_agent_claim" action.subjectId)" == "$partial_gate" ]]
same_agent_action="$(json_field "$same_agent_claim" action.id)"
same_agent_credentials="$(lease_credentials "$same_agent_claim" "$same_agent_worker_id" \
  "$same_agent_worker_token" "$same_agent_lease_token")"
same_agent_report="$(review_report_for_claim "$same_agent_claim")"
same_agent_body="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b["result"]=json.loads(sys.argv[2]); print(json.dumps(b))' \
  "$same_agent_credentials" "$same_agent_report")"
same_agent_status="$(curl -sS -o "$review_tmp/same-agent.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$same_agent_body" \
  "$review_base/api/v1/scheduler/action-runs/$same_agent_action/complete")"
[[ "$same_agent_status" == 409 ]]
[[ "$(json_field "$(<"$review_tmp/same-agent.json")" code)" == independent_reviewer_required ]]
same_agent_fail="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b.update({"failureKind":"transient",
 "summary":"身份不满足独立审核，安全释放给其他 Worker","detail":{"candidateUnchanged":True}})
print(json.dumps(b))' "$same_agent_credentials")"
curl -fsS -H 'content-type: application/json' -d "$same_agent_fail" \
  "$review_base/api/v1/scheduler/action-runs/$same_agent_action/fail" >/dev/null
stale_review_status="$(curl -sS -o "$review_tmp/stale-review.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$same_agent_body" \
  "$review_base/api/v1/scheduler/action-runs/$same_agent_action/complete")"
[[ "$stale_review_status" == 403 ]]
[[ "$(json_field "$(<"$review_tmp/stale-review.json")" code)" == invalid_action_lease ]]
review_candidate "$partial_project_id" "$review_worker_id" "$review_worker_token" \
  partial_review_lease_token_0123456789abcdef
partial_human="$(post_goal_for "$partial_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"partial_accept",
 "rationale":"只采用第一个独立结果，明确舍弃第二项",
 "selectedContributionIds":[sys.argv[2]]}))' "$partial_gate" "$partial_contribution_a")")"
partial_integration="$(json_field "$partial_human" result.integrationId)"
partial_claim="$(claim_action "$integration_worker_id" "$integration_worker_token" \
  partial_integration_lease_token_0123456789abcdef)"
[[ "$(json_field "$partial_claim" action.subjectId)" == "$partial_integration" ]]
partial_action="$(json_field "$partial_claim" action.id)"
partial_credentials="$(lease_credentials "$partial_claim" "$integration_worker_id" \
  "$integration_worker_token" partial_integration_lease_token_0123456789abcdef)"
partial_prepare="$(curl -fsS -H 'content-type: application/json' -d "$partial_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$partial_action/integrations/$partial_integration/prepare")"
partial_key="$(json_field "$partial_prepare" preparationKey)"
partial_candidate="$(json_field "$partial_prepare" candidateCommit)"
partial_tree="$(json_field "$partial_prepare" candidateTreeId)"
partial_snapshot="$(json_field "$partial_prepare" candidateWorkspaceSnapshot)"
[[ "$(<"$review_tmp/runner/$partial_key/chosen/keep.txt")" == keep-this ]]
[[ ! -e "$review_tmp/runner/$partial_key/discard/drop.txt" ]]
[[ ! -e "$review_tmp/worktrees/$partial_root_key/chosen/keep.txt" ]]
partial_validation="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"candidateCommit":sys.argv[1],"candidateTreeId":sys.argv[2],
 "candidateWorkspaceSnapshot":sys.argv[3],"status":"passed",
 "checks":["选中提交存在且未选提交不存在","父目标最小回归通过"],
 "contractCheck":{"desiredOutcome":"passed","hardConstraints":"passed"},
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
  "$partial_candidate" "$partial_tree" "$partial_snapshot")"
partial_finalize_body="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b["validation"]=json.loads(sys.argv[2]); print(json.dumps(b))' \
  "$partial_credentials" "$partial_validation")"
curl -fsS -H 'content-type: application/json' -d "$partial_finalize_body" \
  "$review_base/api/v1/scheduler/action-runs/$partial_action/integrations/$partial_integration/finalize" \
  >/dev/null
[[ "$(<"$review_tmp/worktrees/$partial_root_key/chosen/keep.txt")" == keep-this ]]
[[ ! -e "$review_tmp/worktrees/$partial_root_key/discard/drop.txt" ]]
partial_final_graph="$(curl -fsS "$review_base/api/v1/projects/$partial_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); child=sys.argv[2]; parent=sys.argv[3]; integration=sys.argv[4]
assert next(x for x in s["branches"] if x["id"]==child)["status"]=="stopped"
assert next(x for x in s["branches"] if x["id"]==parent)["status"]=="active"
i=next(x for x in s["integrations"] if x["id"]==integration)
assert i["kind"]=="partial" and i["gitIntegrationStatus"]=="applied"
assert len([x for x in s["integrationContributions"] if x["integrationId"]==integration])==1' \
  "$partial_final_graph" "$partial_child_branch" "$partial_root_branch" "$partial_integration"
[[ "$(docker exec "$review_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM goal_context_entries e JOIN goal_context_snapshot_entries m ON m.entry_id=e.id JOIN goal_sessions s ON s.context_snapshot_id=m.snapshot_id WHERE s.id='$partial_root_session' AND e.origin_goal_branch_id='$partial_child_branch' AND e.source_kind='contribution'")" == 1 ]]

# Selecting a later commit without its prerequisite must not smuggle the first
# commit in through branch ancestry.  Here that exact selection causes a real
# cherry-pick conflict and the parent remains at its original safe point.
conflict_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证选择性集成冲突安全暂停"}' "$review_base/api/projects")"
conflict_project_id="$(json_field "$conflict_project" id)"
conflict_root_proposal_response="$(post_goal_for "$conflict_project_id" proposal.create \
  "{\"revision\":$review_revision}")"
conflict_root_proposal="$(json_field "$conflict_root_proposal_response" result.proposalId)"
post_goal_for "$conflict_project_id" proposal.submit \
  "{\"proposalId\":\"$conflict_root_proposal\",\"expectedRevision\":1}" >/dev/null
conflict_root_approval="$(post_goal_for "$conflict_project_id" proposal.approve \
  "{\"proposalId\":\"$conflict_root_proposal\",\"expectedRevision\":1,\"branchName\":\"冲突父目标\",\"assignment\":\"保持父安全点\",\"agentIdentity\":\"conflict-parent-agent\"}")"
conflict_root_branch="$(json_field "$conflict_root_approval" result.goalBranchId)"
conflict_root_session="$(json_field "$conflict_root_approval" result.sessionId)"
conflict_root_key="$(json_field "$conflict_root_approval" result.workspace.workspace.worktreeKey)"
conflict_root_snapshot="$(json_field "$conflict_root_approval" result.workspace.workspace.workspaceSnapshot)"
conflict_root_write="$(run_workspace_write "$conflict_project_id" "$conflict_root_session" \
  "$conflict_root_key" "$conflict_root_snapshot" "shared/value.txt" "base-value" conflict-root)"
conflict_parent_head="$(json_field "$conflict_root_write" headCommit)"
conflict_child_proposal_response="$(post_goal_for "$conflict_project_id" session.propose_child \
  "{\"parentSessionId\":\"$conflict_root_session\",\"revision\":$review_revision}")"
conflict_child_proposal="$(json_field "$conflict_child_proposal_response" result.proposalId)"
conflict_child_approval="$(post_goal_for "$conflict_project_id" proposal.approve \
  "{\"proposalId\":\"$conflict_child_proposal\",\"expectedRevision\":1,\"branchName\":\"冲突子目标\",\"assignment\":\"形成有先后依赖的提交\",\"agentIdentity\":\"conflict-child-agent\"}")"
conflict_child_branch="$(json_field "$conflict_child_approval" result.goalBranchId)"
conflict_child_session="$(json_field "$conflict_child_approval" result.sessionId)"
conflict_child_contract="$(json_field "$conflict_child_approval" result.contractVersionId)"
conflict_child_key="$(json_field "$conflict_child_approval" result.workspace.workspace.worktreeKey)"
conflict_child_snapshot="$(json_field "$conflict_child_approval" result.workspace.workspace.workspaceSnapshot)"
conflict_write_a="$(run_workspace_write "$conflict_project_id" "$conflict_child_session" \
  "$conflict_child_key" "$conflict_child_snapshot" "shared/value.txt" "intermediate-value" conflict-a)"
conflict_child_snapshot="$(json_field "$conflict_write_a" workspaceSnapshot)"
run_workspace_write "$conflict_project_id" "$conflict_child_session" \
  "$conflict_child_key" "$conflict_child_snapshot" "shared/value.txt" "final-value" conflict-b \
  >/dev/null
conflict_graph="$(curl -fsS "$review_base/api/v1/projects/$conflict_project_id/goal-graph")"
conflict_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$conflict_graph" "$conflict_child_branch")"
conflict_contribution_b="$(json_field "$conflict_contributions" 1)"
conflict_merge="$(post_goal_for "$conflict_project_id" merge.propose \
  "$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":json.loads(sys.argv[2]),
 "evidenceIds":[],"contractVersionId":sys.argv[3],"gitBaseCommit":None,"gitHeadCommit":None,
 "gitDirty":False,"environmentFingerprint":None,"testEvidence":["提交依赖顺序已冻结"],
 "risks":["后置提交可能不能独立应用"],"selfCheck":"完整候选已冻结，等待部分接受验证"}}))' \
    "$conflict_child_session" "$conflict_contributions" "$conflict_child_contract")")"
conflict_gate="$(json_field "$conflict_merge" result.reviewGateId)"
review_candidate "$conflict_project_id" "$review_worker_id" "$review_worker_token" \
  conflict_review_lease_token_0123456789abcdef
conflict_human="$(post_goal_for "$conflict_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"partial_accept",
 "rationale":"故意只选择依赖前一提交的后置结果以验证冲突边界",
 "selectedContributionIds":[sys.argv[2]]}))' "$conflict_gate" "$conflict_contribution_b")")"
conflict_integration="$(json_field "$conflict_human" result.integrationId)"
conflict_claim="$(claim_action "$integration_worker_id" "$integration_worker_token" \
  conflict_integration_lease_token_0123456789abcdef)"
conflict_action="$(json_field "$conflict_claim" action.id)"
conflict_credentials="$(lease_credentials "$conflict_claim" "$integration_worker_id" \
  "$integration_worker_token" conflict_integration_lease_token_0123456789abcdef)"
conflict_prepare_status="$(curl -sS -o "$review_tmp/conflict-prepare.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$conflict_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$conflict_action/integrations/$conflict_integration/prepare")"
[[ "$conflict_prepare_status" == 409 ]]
[[ "$(json_field "$(<"$review_tmp/conflict-prepare.json")" code)" == integration_conflict ]]
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$conflict_root_key" rev-parse HEAD)" \
  == "$conflict_parent_head" ]]
[[ "$(<"$review_tmp/worktrees/$conflict_root_key/shared/value.txt")" == base-value ]]
conflict_state="$(docker exec "$review_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT git_integration_status FROM goal_integrations WHERE id='$conflict_integration'),
          (SELECT status FROM goal_action_runs WHERE id='$conflict_action'),
          (SELECT status FROM goal_workspaces WHERE goal_branch_id='$conflict_root_branch'),
          (SELECT count(*) FROM goal_attention_items WHERE project_id='$conflict_project_id' AND kind='integration_conflict' AND status='open'),
          (SELECT count(*) FROM goal_notifications WHERE project_id='$conflict_project_id' AND action_run_id='$conflict_action' AND status='unread');")"
[[ "$conflict_state" == 'conflicted|waiting|ready|1|1' ]]

# Force the exact cross-storage crash window: Git publish succeeds, then a
# database trigger aborts the transaction before Integration can be recorded as
# applied.  After a real web-container restart, prepare/finalize must recognize
# the candidate already on the ref and finish the same operation exactly once.
recovery_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证 Git 已发布而数据库未确认时的重启恢复"}' "$review_base/api/projects")"
recovery_project_id="$(json_field "$recovery_project" id)"
recovery_root_proposal_response="$(post_goal_for "$recovery_project_id" proposal.create \
  "{\"revision\":$review_revision}")"
recovery_root_proposal="$(json_field "$recovery_root_proposal_response" result.proposalId)"
post_goal_for "$recovery_project_id" proposal.submit \
  "{\"proposalId\":\"$recovery_root_proposal\",\"expectedRevision\":1}" >/dev/null
recovery_root_approval="$(post_goal_for "$recovery_project_id" proposal.approve \
  "{\"proposalId\":\"$recovery_root_proposal\",\"expectedRevision\":1,\"branchName\":\"恢复父目标\",\"assignment\":\"验证跨存储恢复\",\"agentIdentity\":\"recovery-parent-agent\"}")"
recovery_root_branch="$(json_field "$recovery_root_approval" result.goalBranchId)"
recovery_root_session="$(json_field "$recovery_root_approval" result.sessionId)"
recovery_root_key="$(json_field "$recovery_root_approval" result.workspace.workspace.worktreeKey)"
recovery_child_proposal_response="$(post_goal_for "$recovery_project_id" session.propose_child \
  "{\"parentSessionId\":\"$recovery_root_session\",\"revision\":$review_revision}")"
recovery_child_proposal="$(json_field "$recovery_child_proposal_response" result.proposalId)"
recovery_child_approval="$(post_goal_for "$recovery_project_id" proposal.approve \
  "{\"proposalId\":\"$recovery_child_proposal\",\"expectedRevision\":1,\"branchName\":\"恢复子目标\",\"assignment\":\"形成可重放候选\",\"agentIdentity\":\"recovery-child-agent\"}")"
recovery_child_branch="$(json_field "$recovery_child_approval" result.goalBranchId)"
recovery_child_session="$(json_field "$recovery_child_approval" result.sessionId)"
recovery_child_contract="$(json_field "$recovery_child_approval" result.contractVersionId)"
recovery_child_key="$(json_field "$recovery_child_approval" result.workspace.workspace.worktreeKey)"
recovery_child_snapshot="$(json_field "$recovery_child_approval" result.workspace.workspace.workspaceSnapshot)"
run_workspace_write "$recovery_project_id" "$recovery_child_session" \
  "$recovery_child_key" "$recovery_child_snapshot" "recover/value.txt" "recover-once" recovery-child \
  >/dev/null
recovery_graph="$(curl -fsS "$review_base/api/v1/projects/$recovery_project_id/goal-graph")"
recovery_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$recovery_graph" "$recovery_child_branch")"
recovery_merge="$(post_goal_for "$recovery_project_id" merge.propose \
  "$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":json.loads(sys.argv[2]),
 "evidenceIds":[],"contractVersionId":sys.argv[3],"gitBaseCommit":None,"gitHeadCommit":None,
 "gitDirty":False,"environmentFingerprint":None,"testEvidence":["恢复候选已冻结"],
 "risks":["模拟数据库确认失败"],"selfCheck":"候选可安全幂等重放"}}))' \
    "$recovery_child_session" "$recovery_contributions" "$recovery_child_contract")")"
recovery_gate="$(json_field "$recovery_merge" result.reviewGateId)"
review_candidate "$recovery_project_id" "$review_worker_id" "$review_worker_token" \
  recovery_review_lease_token_0123456789abcdef
recovery_human="$(post_goal_for "$recovery_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"accept",
 "rationale":"授权执行可恢复集成","selectedContributionIds":json.loads(sys.argv[2])}))' \
    "$recovery_gate" "$recovery_contributions")")"
recovery_integration="$(json_field "$recovery_human" result.integrationId)"
recovery_claim="$(claim_action "$integration_worker_id" "$integration_worker_token" \
  recovery_integration_lease_token_0123456789abcdef)"
recovery_action="$(json_field "$recovery_claim" action.id)"
recovery_credentials="$(lease_credentials "$recovery_claim" "$integration_worker_id" \
  "$integration_worker_token" recovery_integration_lease_token_0123456789abcdef)"
recovery_prepare="$(curl -fsS -H 'content-type: application/json' -d "$recovery_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$recovery_action/integrations/$recovery_integration/prepare")"
recovery_key="$(json_field "$recovery_prepare" preparationKey)"
recovery_candidate="$(json_field "$recovery_prepare" candidateCommit)"
recovery_tree="$(json_field "$recovery_prepare" candidateTreeId)"
recovery_snapshot="$(json_field "$recovery_prepare" candidateWorkspaceSnapshot)"
recovery_validation="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"candidateCommit":sys.argv[1],"candidateTreeId":sys.argv[2],
 "candidateWorkspaceSnapshot":sys.argv[3],"status":"passed",
 "checks":["候选内容正确","父目标回归通过"],
 "contractCheck":{"desiredOutcome":"passed","hardConstraints":"passed"},
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
  "$recovery_candidate" "$recovery_tree" "$recovery_snapshot")"
recovery_finalize_body="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b["validation"]=json.loads(sys.argv[2]); print(json.dumps(b))' \
  "$recovery_credentials" "$recovery_validation")"
docker exec -i "$review_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test >/dev/null <<'SQL'
CREATE OR REPLACE FUNCTION test_fail_integration_applied()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'injected database confirmation failure';
END;
$$;
CREATE TRIGGER aa_test_fail_integration_applied
  BEFORE UPDATE ON goal_integrations
  FOR EACH ROW WHEN (NEW.git_integration_status = 'applied')
  EXECUTE FUNCTION test_fail_integration_applied();
SQL
recovery_first_status="$(curl -sS -o "$review_tmp/recovery-first.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$recovery_finalize_body" \
  "$review_base/api/v1/scheduler/action-runs/$recovery_action/integrations/$recovery_integration/finalize")"
[[ "$recovery_first_status" == 500 ]]
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$recovery_root_key" rev-parse HEAD)" \
  == "$recovery_candidate" ]]
[[ ! -d "$review_tmp/runner/$recovery_key" ]]
recovery_split_state="$(docker exec "$review_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT git_integration_status FROM goal_integrations WHERE id='$recovery_integration'),
          (SELECT status FROM goal_action_runs WHERE id='$recovery_action'),
          (SELECT status FROM goal_workspaces WHERE goal_branch_id='$recovery_root_branch');")"
[[ "$recovery_split_state" == 'validating|running|applying' ]]
docker exec -i "$review_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test >/dev/null <<'SQL'
DROP TRIGGER aa_test_fail_integration_applied ON goal_integrations;
DROP FUNCTION test_fail_integration_applied();
SQL
docker restart "$review_app" >/dev/null
review_port="$(docker port "$review_app" 3000/tcp | sed -n 's/.*://p')"
review_base="http://127.0.0.1:$review_port"
for review_attempt in $(seq 1 60); do
  curl --connect-timeout 1 --max-time 2 -fsS "$review_base/api/health" >/dev/null 2>&1 && break
  [[ "$review_attempt" == 60 ]] && docker logs "$review_app" && exit 1
  sleep 0.5
done
recovery_reprepare="$(curl -fsS -H 'content-type: application/json' -d "$recovery_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$recovery_action/integrations/$recovery_integration/prepare")"
[[ "$(json_field "$recovery_reprepare" replayed)" == true ]]
[[ "$(json_field "$recovery_reprepare" candidateCommit)" == "$recovery_candidate" ]]
recovery_final="$(curl -fsS -H 'content-type: application/json' -d "$recovery_finalize_body" \
  "$review_base/api/v1/scheduler/action-runs/$recovery_action/integrations/$recovery_integration/finalize")"
[[ "$(json_field "$recovery_final" status)" == applied ]]
[[ "$(<"$review_tmp/worktrees/$recovery_root_key/recover/value.txt")" == recover-once ]]
recovery_final_state="$(docker exec "$review_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT git_integration_status FROM goal_integrations WHERE id='$recovery_integration'),
          (SELECT status FROM goal_action_runs WHERE id='$recovery_action'),
          (SELECT status FROM goal_branches WHERE id='$recovery_child_branch'),
          (SELECT count(*) FROM workspace_snapshots WHERE operation_id=(SELECT operation_id FROM goal_integrations WHERE id='$recovery_integration'));")"
[[ "$recovery_final_state" == 'applied|succeeded|integrated|1' ]]

# A competing clean parent commit between validation and publish is a real CAS
# race.  The system must preserve that winner, never reset it to the candidate,
# and move the Integration into an explicit human-attention state.
race_project="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证父 ref 抢先移动时不覆盖胜者"}' "$review_base/api/projects")"
race_project_id="$(json_field "$race_project" id)"
race_root_proposal_response="$(post_goal_for "$race_project_id" proposal.create \
  "{\"revision\":$review_revision}")"
race_root_proposal="$(json_field "$race_root_proposal_response" result.proposalId)"
post_goal_for "$race_project_id" proposal.submit \
  "{\"proposalId\":\"$race_root_proposal\",\"expectedRevision\":1}" >/dev/null
race_root_approval="$(post_goal_for "$race_project_id" proposal.approve \
  "{\"proposalId\":\"$race_root_proposal\",\"expectedRevision\":1,\"branchName\":\"CAS 父目标\",\"assignment\":\"保留并发胜者\",\"agentIdentity\":\"race-parent-agent\"}")"
race_root_branch="$(json_field "$race_root_approval" result.goalBranchId)"
race_root_session="$(json_field "$race_root_approval" result.sessionId)"
race_root_key="$(json_field "$race_root_approval" result.workspace.workspace.worktreeKey)"
race_expected_parent="$(json_field "$race_root_approval" result.workspace.workspace.headCommit)"
race_child_proposal_response="$(post_goal_for "$race_project_id" session.propose_child \
  "{\"parentSessionId\":\"$race_root_session\",\"revision\":$review_revision}")"
race_child_proposal="$(json_field "$race_child_proposal_response" result.proposalId)"
race_child_approval="$(post_goal_for "$race_project_id" proposal.approve \
  "{\"proposalId\":\"$race_child_proposal\",\"expectedRevision\":1,\"branchName\":\"CAS 子目标\",\"assignment\":\"形成待 CAS 结果\",\"agentIdentity\":\"race-child-agent\"}")"
race_child_branch="$(json_field "$race_child_approval" result.goalBranchId)"
race_child_session="$(json_field "$race_child_approval" result.sessionId)"
race_child_contract="$(json_field "$race_child_approval" result.contractVersionId)"
race_child_key="$(json_field "$race_child_approval" result.workspace.workspace.worktreeKey)"
race_child_snapshot="$(json_field "$race_child_approval" result.workspace.workspace.workspaceSnapshot)"
run_workspace_write "$race_project_id" "$race_child_session" "$race_child_key" \
  "$race_child_snapshot" "race/candidate.txt" "candidate-must-not-win" race-child >/dev/null
race_graph="$(curl -fsS "$review_base/api/v1/projects/$race_project_id/goal-graph")"
race_contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
  "$race_graph" "$race_child_branch")"
race_merge="$(post_goal_for "$race_project_id" merge.propose \
  "$(python3 -c 'import json,sys
print(json.dumps({"sessionId":sys.argv[1],"candidate":{"contributionIds":json.loads(sys.argv[2]),
 "evidenceIds":[],"contractVersionId":sys.argv[3],"gitBaseCommit":None,"gitHeadCommit":None,
 "gitDirty":False,"environmentFingerprint":None,"testEvidence":["CAS 候选已冻结"],
 "risks":["父 ref 可能抢先变化"],"selfCheck":"最终发布必须比较准确父 HEAD"}}))' \
    "$race_child_session" "$race_contributions" "$race_child_contract")")"
race_gate="$(json_field "$race_merge" result.reviewGateId)"
review_candidate "$race_project_id" "$review_worker_id" "$review_worker_token" \
  race_review_lease_token_0123456789abcdef
race_human="$(post_goal_for "$race_project_id" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"accept",
 "rationale":"授权 CAS 集成但不得覆盖并发父提交","selectedContributionIds":json.loads(sys.argv[2])}))' \
    "$race_gate" "$race_contributions")")"
race_integration="$(json_field "$race_human" result.integrationId)"
race_claim="$(claim_action "$integration_worker_id" "$integration_worker_token" \
  race_integration_lease_token_0123456789abcdef)"
race_action="$(json_field "$race_claim" action.id)"
race_credentials="$(lease_credentials "$race_claim" "$integration_worker_id" \
  "$integration_worker_token" race_integration_lease_token_0123456789abcdef)"
race_prepare="$(curl -fsS -H 'content-type: application/json' -d "$race_credentials" \
  "$review_base/api/v1/scheduler/action-runs/$race_action/integrations/$race_integration/prepare")"
race_candidate="$(json_field "$race_prepare" candidateCommit)"
race_tree="$(json_field "$race_prepare" candidateTreeId)"
race_snapshot="$(json_field "$race_prepare" candidateWorkspaceSnapshot)"
docker exec "$review_app" git -C "/data/worktrees/$race_root_key" \
  -c user.name=Fudian-Race -c user.email=race@fudian.invalid \
  commit --allow-empty --no-gpg-sign -m 'competing parent winner' >/dev/null
race_winner="$(docker exec "$review_app" git -C "/data/worktrees/$race_root_key" rev-parse HEAD)"
[[ "$race_winner" != "$race_expected_parent" && "$race_winner" != "$race_candidate" ]]
race_validation="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"candidateCommit":sys.argv[1],"candidateTreeId":sys.argv[2],
 "candidateWorkspaceSnapshot":sys.argv[3],"status":"passed","checks":["候选回归通过"],
 "contractCheck":{"desiredOutcome":"passed","hardConstraints":"passed"},
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
  "$race_candidate" "$race_tree" "$race_snapshot")"
race_finalize_body="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b["validation"]=json.loads(sys.argv[2]); print(json.dumps(b))' \
  "$race_credentials" "$race_validation")"
race_finalize_status="$(curl -sS -o "$review_tmp/race-finalize.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$race_finalize_body" \
  "$review_base/api/v1/scheduler/action-runs/$race_action/integrations/$race_integration/finalize")"
[[ "$race_finalize_status" == 409 ]]
[[ "$(json_field "$(<"$review_tmp/race-finalize.json")" code)" == integration_publish_conflict ]]
[[ "$(docker exec "$review_app" git -C "/data/worktrees/$race_root_key" rev-parse HEAD)" \
  == "$race_winner" ]]
[[ ! -e "$review_tmp/worktrees/$race_root_key/race/candidate.txt" ]]
race_state="$(docker exec "$review_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT git_integration_status FROM goal_integrations WHERE id='$race_integration'),
          (SELECT status FROM goal_action_runs WHERE id='$race_action'),
          (SELECT status FROM goal_workspaces WHERE goal_branch_id='$race_root_branch'),
          (SELECT count(*) FROM goal_attention_items WHERE project_id='$race_project_id' AND kind='integration_conflict' AND status='open');")"
[[ "$race_state" == 'conflicted|waiting|error|1' ]]

echo "review/integration HTTP flow passed: frozen candidate, leased independent review, full/partial CAS integration, safe conflict/CAS pause, crash-window recovery, context resume and human-only root completion"
