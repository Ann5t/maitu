#!/usr/bin/env bash
set -euo pipefail

# End-to-end proof for the REAL maitu-review-worker binary (not the curl
# stand-in used by test-review-integration-http.sh): the worker registers an
# independent identity, claims review.goal_candidate.v1 actions, re-observes a
# frozen candidate read-only, and submits a bound report that moves the gate to
# pending_human_review.  A tampered sibling candidate must be reported as
# unsafe_state (waiting for a human) instead of being approved.
#
# The worker container mirrors production isolation: candidate repositories and
# worktrees mounted read-only, no Docker socket, cap_drop ALL,
# no-new-privileges.  The worker itself refuses to attest if any of those are
# missing, so a passing review here also proves the isolation probe is honest.

rw_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rw_suffix="$$"
rw_network="fudian-review-worker-test-$rw_suffix"
rw_db="fudian-review-worker-db-$rw_suffix"
rw_app="fudian-review-worker-app-$rw_suffix"
rw_worker="fudian-review-worker-worker-$rw_suffix"
rw_tmp="$(mktemp -d)"
rw_bootstrap="bootstrap_review_worker_0123456789abcdef"

# shellcheck source=scripts/docker-test-lib.sh
. "$rw_repo_root/scripts/docker-test-lib.sh"

cleanup_rw_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )); then
    if docker inspect "$rw_worker" >/dev/null 2>&1; then
      echo "---- worker 日志（失败时） ----" >&2
      docker logs "$rw_worker" >&2 || true
    fi
    if docker inspect "$rw_app" >/dev/null 2>&1; then
      echo "---- app 日志尾部（失败时） ----" >&2
      docker logs "$rw_app" | tail -n 40 >&2 || true
    fi
    if docker inspect "$rw_db" >/dev/null 2>&1; then
      echo "---- 关键表状态（失败时） ----" >&2
      docker exec "$rw_db" psql -U fudian_test -d fudian_test -c \
        "SELECT id, status, last_error_code FROM goal_action_runs ORDER BY created_at" >&2 || true
      docker exec "$rw_db" psql -U fudian_test -d fudian_test -c \
        "SELECT g.id, g.status, d.actor_role, d.decision FROM goal_review_gates g \
         LEFT JOIN goal_review_decisions d ON d.review_gate_id = g.id ORDER BY g.created_at" >&2 || true
      docker exec "$rw_db" psql -U fudian_test -d fudian_test -c \
        "SELECT id, status FROM goal_branches ORDER BY created_at" >&2 || true
    fi
  fi
  [[ "$rw_worker" == fudian-review-worker-worker-* ]] \
    && docker rm -fv "$rw_worker" >/dev/null 2>&1 || true
  [[ "$rw_app" == fudian-review-worker-app-* ]] \
    && docker rm -fv "$rw_app" >/dev/null 2>&1 || true
  [[ "$rw_db" == fudian-review-worker-db-* ]] \
    && docker rm -fv "$rw_db" >/dev/null 2>&1 || true
  [[ "$rw_network" == fudian-review-worker-test-* ]] \
    && docker network rm "$rw_network" >/dev/null 2>&1 || true
  [[ "$rw_tmp" == /tmp/tmp.* && -d "$rw_tmp" ]] \
    && fudian_test_remove_bind_tree "$rw_tmp"
  return "$exit_status"
}
trap cleanup_rw_stack EXIT

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
  local status
  body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":str(__import__("uuid").uuid4()),
 "action":sys.argv[1],"payload":json.loads(sys.argv[2])},ensure_ascii=False))' \
      "$action" "$payload")"
  status="$(curl -sS -o "$rw_tmp/goal-command-response.json" -w '%{http_code}' \
    -H 'content-type: application/json' -d "$body" \
    "$rw_base/api/v1/projects/$project_id/goal-commands")"
  if [[ "$status" != 2* ]]; then
    echo "goal 命令 $action 返回 HTTP $status：$(cat "$rw_tmp/goal-command-response.json")" >&2
    return 1
  fi
  cat "$rw_tmp/goal-command-response.json"
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
    "$rw_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs"
}

execute_runner_job() {
  local prepared="$1"
  local worktree_key="$2"
  local label="$3"
  local spec_file="$rw_tmp/$label-spec.json"
  local output_key
  output_key="$(json_field "$prepared" outputKey)"
  python3 -c 'import json,sys
with open(sys.argv[2],"w",encoding="utf-8") as handle:
 json.dump(json.loads(sys.argv[1])["spec"],handle,separators=(",",":"),ensure_ascii=False)' \
    "$prepared" "$spec_file"
  fudian_test_open_runner_output "$rw_app" "$output_key"
  docker run --rm --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges:true --pids-limit 32 --memory 128m --cpus 0.5 \
    --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
    --mount "type=bind,src=$rw_tmp/worktrees/$worktree_key,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$rw_tmp/runner/$output_key,dst=/workspace/output" \
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
    "$rw_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs/$job_id/finalize"
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
  prepared="$(prepare_runner_job "$project_id" "$session_id" "$snapshot" "$relative_path" "$content")"
  result="$(execute_runner_job "$prepared" "$worktree_key" "$label")"
  finalize_runner_job "$project_id" "$session_id" "$prepared" "$result"
}

rw_revision='{
  "whyNeeded":"验证真实 Review Worker 二进制驱动评审闸门",
  "contract":{
    "desiredOutcome":"独立审核后由用户决定采用",
    "hardConstraints":["候选必须只读复核","漂移必须等待人工"],
    "subjectivePreferences":[],"unknowns":[],"nonGoals":["不推送远程仓库"],
    "validationPlan":["只读复验 Git 冻结现场"],
    "judgmentTriggers":[],
    "stopConditions":["冻结候选与观察一致"],
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

# 建立一个带真实 worktree 与一次真实写入的目标枝干，并返回拟合并载荷
# （Git 事实取自服务端行，禁用占位值：拟合并必须绑定真实安全点）。
setup_goal_with_merge() {
  local label="$1"
  local agent_identity="$2"
  local write_path="$3"
  local write_content="$4"
  local project
  local project_id
  local proposal_response
  local proposal_id
  local approval
  local branch_id
  local session_id
  local worktree_key
  local snapshot
  local write
  local graph
  local contract_id
  local contributions
  local row
  project="$(curl -fsS -H 'content-type: application/json' \
    -d "{\"intent\":\"验证真实 Review Worker（$label）\"}" "$rw_base/api/projects")"
  project_id="$(json_field "$project" id)"
  proposal_response="$(post_goal_for "$project_id" proposal.create "{\"revision\":$rw_revision}")"
  proposal_id="$(json_field "$proposal_response" result.proposalId)"
  post_goal_for "$project_id" proposal.submit \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1}" >/dev/null
  approval="$(post_goal_for "$project_id" proposal.approve \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1,\"branchName\":\"$label\",\"assignment\":\"形成一次真实写入\",\"agentIdentity\":\"$agent_identity\"}")"
  branch_id="$(json_field "$approval" result.goalBranchId)"
  session_id="$(json_field "$approval" result.sessionId)"
  worktree_key="$(json_field "$approval" result.workspace.workspace.worktreeKey)"
  snapshot="$(json_field "$approval" result.workspace.workspace.workspaceSnapshot)"
  write="$(run_workspace_write "$project_id" "$session_id" "$worktree_key" "$snapshot" \
    "$write_path" "$write_content" "$label")"
  snapshot="$(json_field "$write" workspaceSnapshot)"
  graph="$(curl -fsS "$rw_base/api/v1/projects/$project_id/goal-graph")"
  contract_id="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(next(x for x in s["branches"] if x["id"]==branch)["currentContractVersionId"])' \
    "$graph" "$branch_id")"
  contributions="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]
print(json.dumps([x["id"] for x in s["contributions"] if x["goalBranchId"]==branch]))' \
    "$graph" "$branch_id")"
  row="$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
    "SELECT base_commit, head_commit, tree_id, workspace_snapshot FROM goal_workspaces WHERE goal_branch_id='$branch_id'")"
  python3 - "$project_id" "$branch_id" "$session_id" "$worktree_key" "$row" "$contributions" "$contract_id" <<'PY'
import json, sys
project_id, branch_id, session_id, worktree_key, row, contributions, contract_id = sys.argv[1:8]
base, head, tree, snapshot = row.split("|")
payload = {
    "sessionId": session_id,
    "candidate": {
        "contributionIds": json.loads(contributions),
        "evidenceIds": [],
        "contractVersionId": contract_id,
        "gitBaseCommit": base,
        "gitHeadCommit": head,
        "treeId": tree,
        "workspaceSnapshot": snapshot,
        "gitDirty": False,
        "environmentFingerprint": None,
        "testEvidence": ["Runner 输出已提交"],
        "risks": [],
        "selfCheck": "已逐条检查目标、依赖和代码提交边界",
    },
}
print(json.dumps({
    "projectId": project_id,
    "branchId": branch_id,
    "sessionId": session_id,
    "worktreeKey": worktree_key,
    "payload": payload,
}, ensure_ascii=False))
PY
}

mkdir -p "$rw_tmp/artifacts" "$rw_tmp/repositories" "$rw_tmp/worktrees" \
  "$rw_tmp/runner" "$rw_tmp/executor"
chmod 0777 "$rw_tmp/artifacts" "$rw_tmp/repositories" "$rw_tmp/worktrees" \
  "$rw_tmp/runner" "$rw_tmp/executor"
# bootstrap secret 只在子 shell 里收紧 umask：全局收紧会让后续 spec.json
# 变成 0600，Runner 容器（uid 1000）读不到。
(
  umask 077
  printf '%s' "$rw_bootstrap" > "$rw_tmp/executor/review-bootstrap"
  chmod 400 "$rw_tmp/executor/review-bootstrap"
)

docker network create "$rw_network" >/dev/null
docker run -d --name "$rw_db" --network "$rw_network" --network-alias rw-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for rw_attempt in $(seq 1 30); do
  docker exec "$rw_db" pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test \
    >/dev/null 2>&1 && break
  [[ "$rw_attempt" == 30 ]] && docker logs "$rw_db" && exit 1
  sleep 1
done

rw_runner_digest="$(docker run --rm --entrypoint /usr/local/bin/fudian-runner \
  fudian-nextgen-runner:latest digest)"
docker run -d --name "$rw_app" --network "$rw_network" --network-alias rw-app \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@rw-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled \
  -e FUDIAN_BIND=0.0.0.0:3000 -e ARTIFACT_ROOT=/data/artifacts \
  -e REPOSITORY_ROOT=/data/repositories -e WORKTREE_ROOT=/data/worktrees \
  -e RUNNER_OUTPUT_ROOT=/data/runner -e RUNNER_RUNTIME_DIGEST="$rw_runner_digest" \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$rw_bootstrap" -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$rw_repo_root,dst=/app" \
  --mount "type=bind,src=$rw_tmp/artifacts,dst=/data/artifacts" \
  --mount "type=bind,src=$rw_tmp/repositories,dst=/data/repositories" \
  --mount "type=bind,src=$rw_tmp/worktrees,dst=/data/worktrees" \
  --mount "type=bind,src=$rw_tmp/runner,dst=/data/runner" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run --locked >/dev/null

rw_port="$(docker port "$rw_app" 3000/tcp | sed -n 's/.*://p')"
rw_base="http://127.0.0.1:$rw_port"
for rw_attempt in $(seq 1 60); do
  curl -fsS "$rw_base/api/health" >/dev/null 2>&1 && break
  if [[ "$rw_attempt" == 60 ]]; then docker logs "$rw_app"; exit 1; fi
  sleep 1
done

# 场景 A：完整候选。场景 B：冻结后从宿主侧篡改现场。两者都在 Review
# Worker 启动之前就绪，避免与领取循环竞速。
rw_setup_a="$(setup_goal_with_merge intact alpha-agent "feature/intact.txt" intact-result)"
rw_setup_b="$(setup_goal_with_merge tampered beta-agent "feature/tampered.txt" tampered-result)"
rw_project_a="$(json_field "$rw_setup_a" projectId)"
rw_branch_a="$(json_field "$rw_setup_a" branchId)"
rw_key_b="$(json_field "$rw_setup_b" worktreeKey)"
rw_project_b="$(json_field "$rw_setup_b" projectId)"
rw_merge_a="$(post_goal_for "$rw_project_a" merge.propose \
  "$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])["payload"]))' "$rw_setup_a")")"
rw_gate_a="$(json_field "$rw_merge_a" result.reviewGateId)"
rw_merge_b="$(post_goal_for "$rw_project_b" merge.propose \
  "$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])["payload"]))' "$rw_setup_b")")"
rw_gate_b="$(json_field "$rw_merge_b" result.reviewGateId)"
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_review_gates WHERE id = '$rw_gate_a'")" == pending_ai_review ]]
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_review_gates WHERE id = '$rw_gate_b'")" == pending_ai_review ]]

# 冻结后在宿主侧写入未跟踪文件：只读观察必须发现漂移。
printf 'out-of-band drift\n' > "$rw_tmp/worktrees/$rw_key_b/drift.tmp"

docker run -d --name "$rw_worker" --network "$rw_network" \
  -e MAITU_REVIEW_BASE_URL=http://rw-app:3000 -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$rw_repo_root,dst=/app" \
  --mount "type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry" \
  --mount "type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git" \
  --mount "type=volume,src=fudian_rust_target,dst=/app/target" \
  --mount "type=bind,src=$rw_tmp/repositories,dst=/data/repositories,readonly" \
  --mount "type=bind,src=$rw_tmp/worktrees,dst=/data/worktrees,readonly" \
  --mount "type=bind,src=$rw_tmp/executor/review-bootstrap,dst=/run/maitu-executor/review-bootstrap,readonly" \
  --cap-drop ALL --security-opt no-new-privileges:true --pids-limit 128 \
  fudian-nextgen-app:latest cargo run --locked --bin maitu-review-worker >/dev/null

# 等 Worker 把完整候选推到 pending_human_review（首次编译可能偏慢）。
rw_gate_a_status=""
for rw_attempt in $(seq 1 240); do
  rw_gate_a_status="$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
    "SELECT status FROM goal_review_gates WHERE id = '$rw_gate_a'")"
  [[ "$rw_gate_a_status" == pending_human_review ]] && break
  if ! docker inspect "$rw_worker" --format '{{.State.Running}}' | grep -q true; then
    echo "Review Worker 容器提前退出" >&2
    docker logs "$rw_worker" >&2 || true
    exit 1
  fi
  sleep 1
done
[[ "$rw_gate_a_status" == pending_human_review ]] || {
  echo "闸门 A 未在时限内升格（当前 $rw_gate_a_status）" >&2
  docker logs "$rw_worker" >&2 || true
  exit 1
}

[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT actor_role || ':' || decision FROM goal_review_decisions WHERE review_gate_id = '$rw_gate_a'")" \
  == review_ai:recommend_accept ]]
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT a.status FROM goal_action_runs a WHERE a.subject_id = '$rw_gate_a' AND a.subject_kind = 'review_gate'")" \
  == succeeded ]]
docker logs "$rw_worker" 2>&1 | grep -q "已提交审核报告"

# 等待漂移候选被判为 unsafe_state（waiting 等人工，闸门不升格）。
rw_action_b_status=""
for rw_attempt in $(seq 1 60); do
  rw_action_b_status="$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
    "SELECT a.status FROM goal_action_runs a WHERE a.subject_id = '$rw_gate_b' AND a.subject_kind = 'review_gate'")"
  [[ "$rw_action_b_status" == waiting ]] && break
  sleep 1
done
[[ "$rw_action_b_status" == waiting ]] || {
  echo "漂移候选未被判为 waiting（当前 $rw_action_b_status）" >&2
  docker logs "$rw_worker" >&2 || true
  exit 1
}
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_review_gates WHERE id = '$rw_gate_b'")" == pending_ai_review ]]
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT last_error_code FROM goal_action_runs WHERE subject_id = '$rw_gate_b' AND subject_kind = 'review_gate'")" \
  == unsafe_state ]]
docker logs "$rw_worker" 2>&1 | grep -q "现场已漂移"

# 用户最终决定：接受完整候选，根目标枝干完成闭环。
rw_human="$(post_goal_for "$rw_project_a" review.human_decide \
  "$(python3 -c 'import json,sys
print(json.dumps({"reviewGateId":sys.argv[1],"decision":"accept",
 "rationale":"独立审核报告与冻结证据一致，接受该候选",
 "selectedContributionIds":json.loads(sys.argv[2])["payload"]["candidate"]["contributionIds"]}))' \
    "$rw_gate_a" "$rw_setup_a")")"
[[ "$(json_field "$rw_human" result.gitIntegrationStatus)" == null ]]
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_review_gates WHERE id = '$rw_gate_a'")" == accepted ]]
[[ "$(docker exec "$rw_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT status FROM goal_branches WHERE id = '$(json_field "$rw_setup_a" branchId)'")" \
  == completed ]]

echo "Review Worker 端到端验证通过：完整候选升格待人工并接受，漂移候选被判 unsafe_state 等待人工。"
