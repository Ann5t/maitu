#!/usr/bin/env bash
set -euo pipefail

goal_http_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
goal_http_suffix="$$"
goal_http_network="fudian-goal-http-test-$goal_http_suffix"
goal_http_db="fudian-goal-http-db-$goal_http_suffix"
goal_http_app="fudian-goal-http-app-$goal_http_suffix"

cleanup_goal_http() {
  if [[ "$goal_http_app" == fudian-goal-http-app-* ]]; then
    docker rm -f "$goal_http_app" >/dev/null 2>&1 || true
  fi
  if [[ "$goal_http_db" == fudian-goal-http-db-* ]]; then
    docker rm -f "$goal_http_db" >/dev/null 2>&1 || true
  fi
  if [[ "$goal_http_network" == fudian-goal-http-test-* ]]; then
    docker network rm "$goal_http_network" >/dev/null 2>&1 || true
  fi
}
trap cleanup_goal_http EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys; value=json.loads(sys.argv[1]);
for key in sys.argv[2].split("."):
    value=value[key]
print(str(value).lower() if isinstance(value,bool) else value)' "$1" "$2"
}

post_goal() {
  local goal_action="$1"
  local goal_payload="$2"
  local goal_request_id="$3"
  local goal_body
  goal_body="$(python3 -c 'import json,sys; print(json.dumps({
    "clientRequestId": sys.argv[1],
    "action": sys.argv[2],
    "payload": json.loads(sys.argv[3])
  }, ensure_ascii=False))' "$goal_request_id" "$goal_action" "$goal_payload")"
  echo "goal HTTP command: $goal_action" >&2
  local goal_raw_response
  local goal_status
  local goal_response_body
  goal_raw_response="$(curl -sS -w $'\n%{http_code}' -H 'content-type: application/json' \
    -d "$goal_body" \
    "$goal_http_base/api/v1/projects/$goal_project_id/goal-commands")"
  goal_status="${goal_raw_response##*$'\n'}"
  goal_response_body="${goal_raw_response%$'\n'*}"
  if [[ "$goal_status" -lt 200 || "$goal_status" -ge 300 ]]; then
    echo "goal HTTP command failed ($goal_status): $goal_action: $goal_response_body" >&2
    return 22
  fi
  printf '%s' "$goal_response_body"
}

docker network create "$goal_http_network" >/dev/null
docker run -d --name "$goal_http_db" --network "$goal_http_network" \
  --network-alias goal-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for goal_http_attempt in $(seq 1 30); do
  if docker exec "$goal_http_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  if [[ "$goal_http_attempt" == 30 ]]; then
    docker logs "$goal_http_db"
    exit 1
  fi
  sleep 1
done

docker run -d --name "$goal_http_app" --network "$goal_http_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@goal-db:5432/fudian_test \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/tmp/fudian-goal-http-artifacts \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$goal_http_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null

goal_http_port="$(docker port "$goal_http_app" 3000/tcp | sed -n 's/.*://p')"
goal_http_base="http://127.0.0.1:$goal_http_port"
for goal_http_attempt in $(seq 1 60); do
  if curl -fsS "$goal_http_base/api/health" >/dev/null 2>&1; then
    break
  fi
  if [[ "$goal_http_attempt" == 60 ]]; then
    docker logs "$goal_http_app"
    exit 1
  fi
  sleep 1
done

goal_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"用隔离 HTTP 测试验证目标枝干审核闭环"}' \
  "$goal_http_base/api/projects")"
goal_project_id="$(json_field "$goal_project_response" id)"

goal_root_revision='{
  "whyNeeded":"建立项目的根目标枝干",
  "contract":{
    "desiredOutcome":"目标枝干 HTTP 闭环通过真实请求验证",
    "hardConstraints":["不写入真实数据库"],
    "subjectivePreferences":[],
    "unknowns":["子目标第一次候选是否会通过"],
    "nonGoals":["不公开部署"],
    "validationPlan":["运行完整 HTTP 流程并查询快照"],
    "judgmentTriggers":["拟合并后由用户决定"],
    "stopConditions":["根目标经用户接受"],
    "expectedContributions":["可重复验证的快照"]
  },
  "expectedContributions":["审核事件链"],
  "explorationPlan":["先验证子目标退回和继续"],
  "contextInheritance":{},
  "toolRequirements":[],
  "inferences":["HTTP 200 与数据库状态必须同时成立"],
  "revisionReason":null
}'

goal_response="$(post_goal proposal.create \
  "{\"revision\":$goal_root_revision}" "$(new_uuid)")"
goal_root_proposal_id="$(json_field "$goal_response" result.proposalId)"

goal_revised_revision="$(python3 -c 'import json,sys; value=json.loads(sys.argv[1]);
value["revisionReason"]="补充真实快照验收";
value["contract"]["validationPlan"].append("核对未接受时父枝干无 Integration");
print(json.dumps(value,ensure_ascii=False))' "$goal_root_revision")"
goal_response="$(post_goal proposal.revise \
  "{\"proposalId\":\"$goal_root_proposal_id\",\"expectedRevision\":1,\"revision\":$goal_revised_revision}" \
  "$(new_uuid)")"
[[ "$(json_field "$goal_response" result.revision)" == 2 ]]

post_goal proposal.submit \
  "{\"proposalId\":\"$goal_root_proposal_id\",\"expectedRevision\":2}" \
  "$(new_uuid)" >/dev/null
goal_response="$(post_goal proposal.approve \
  "{\"proposalId\":\"$goal_root_proposal_id\",\"expectedRevision\":2,\"branchName\":\"根目标\",\"assignment\":\"推进并整合子目标\",\"agentIdentity\":\"worker-root\"}" \
  "$(new_uuid)")"
goal_root_branch_id="$(json_field "$goal_response" result.goalBranchId)"
goal_root_session_id="$(json_field "$goal_response" result.sessionId)"
goal_root_contract_id="$(json_field "$goal_response" result.contractVersionId)"

goal_child_revision='{
  "whyNeeded":"独立验证退回后在同一枝干创建下一 Session",
  "contract":{
    "desiredOutcome":"子目标经历一次退回后由下一 Session 修正并被接受",
    "hardConstraints":["父枝干在接受前保持不变"],
    "subjectivePreferences":[],
    "unknowns":["第一轮审核会发现什么"],
    "nonGoals":["不直接修改父 worktree"],
    "validationPlan":["查询 Gate、Session 与 Integration 状态"],
    "judgmentTriggers":["候选冻结后等待用户"],
    "stopConditions":["选中 Contribution 回流父枝干"],
    "expectedContributions":["第一次发现","第二次修正"]
  },
  "expectedContributions":["可审计的修正结果"],
  "explorationPlan":["先提交不完整候选，再按退回要求修正"],
  "contextInheritance":{},
  "toolRequirements":[],
  "inferences":[],
  "revisionReason":null
}'
goal_response="$(post_goal session.propose_child \
  "{\"parentSessionId\":\"$goal_root_session_id\",\"revision\":$goal_child_revision}" \
  "$(new_uuid)")"
goal_child_proposal_id="$(json_field "$goal_response" result.proposalId)"
goal_response="$(post_goal proposal.approve \
  "{\"proposalId\":\"$goal_child_proposal_id\",\"expectedRevision\":1,\"branchName\":\"审核闭环子目标\",\"assignment\":\"提交第一轮候选\",\"agentIdentity\":\"worker-child\"}" \
  "$(new_uuid)")"
goal_child_branch_id="$(json_field "$goal_response" result.goalBranchId)"
goal_child_session_1="$(json_field "$goal_response" result.sessionId)"
goal_child_contract_id="$(json_field "$goal_response" result.contractVersionId)"

goal_response="$(post_goal session.add_contribution \
  "{\"sessionId\":\"$goal_child_session_1\",\"kind\":\"finding\",\"title\":\"第一轮发现\",\"body\":\"流程可运行，但验收证据不够完整\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}" \
  "$(new_uuid)")"
goal_contribution_1="$(json_field "$goal_response" result.contributionId)"
goal_response="$(post_goal merge.propose \
  "{\"sessionId\":\"$goal_child_session_1\",\"candidate\":{\"contributionIds\":[\"$goal_contribution_1\"],\"contractVersionId\":\"$goal_child_contract_id\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"第一轮 HTTP 请求成功\"],\"risks\":[\"证据覆盖不足\"],\"selfCheck\":\"已检查状态，但缺少退回路径证据\"}}" \
  "$(new_uuid)")"
goal_gate_1="$(json_field "$goal_response" result.reviewGateId)"
post_goal review.ai_record \
  "{\"reviewGateId\":\"$goal_gate_1\",\"reviewerIdentity\":\"reviewer-a\",\"decision\":\"recommend_reject\",\"rationale\":\"缺少退回后的继续证据\",\"contractCheck\":{\"missing\":[\"下一 Session\"]},\"retestEvidence\":[]}" \
  "$(new_uuid)" >/dev/null
goal_reject_request_id="$(new_uuid)"
goal_reject_payload="{\"reviewGateId\":\"$goal_gate_1\",\"decision\":\"reject\",\"rationale\":\"补齐下一 Session 与再次审核证据\",\"selectedContributionIds\":[]}"
goal_response="$(post_goal review.human_decide "$goal_reject_payload" "$goal_reject_request_id")"
[[ "$(json_field "$goal_response" result.status)" == rejected ]]
goal_replay_response="$(post_goal review.human_decide "$goal_reject_payload" "$goal_reject_request_id")"
[[ "$(json_field "$goal_replay_response" replayed)" == true ]]

goal_conflict_body="$(python3 -c 'import json,sys; print(json.dumps({
  "clientRequestId":sys.argv[1],"action":"review.human_decide",
  "payload":{"reviewGateId":sys.argv[2],"decision":"reject","rationale":"不同输入","selectedContributionIds":[]}
}))' "$goal_reject_request_id" "$goal_gate_1")"
goal_conflict_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$goal_conflict_body" \
  "$goal_http_base/api/v1/projects/$goal_project_id/goal-commands")"
[[ "$goal_conflict_status" == 409 ]]

goal_snapshot="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1])
root=sys.argv[2]; child=sys.argv[3]
branches={b["id"]:b for b in s["branches"]}
assert branches[root]["status"] == "waiting"
assert branches[child]["status"] == "active"
assert len(s["integrations"]) == 0
assert s["project"]["state"] == "active"' \
  "$goal_snapshot" "$goal_root_branch_id" "$goal_child_branch_id"

goal_response="$(post_goal session.start_next \
  "{\"goalBranchId\":\"$goal_child_branch_id\",\"previousSessionId\":\"$goal_child_session_1\",\"assignment\":\"按退回意见补齐证据\",\"agentIdentity\":\"worker-child\"}" \
  "$(new_uuid)")"
goal_child_session_2="$(json_field "$goal_response" result.sessionId)"
goal_response="$(post_goal session.add_contribution \
  "{\"sessionId\":\"$goal_child_session_2\",\"kind\":\"evidence\",\"title\":\"第二轮修正\",\"body\":\"退回后在同一枝干的新 Session 完成复验\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}" \
  "$(new_uuid)")"
goal_contribution_2="$(json_field "$goal_response" result.contributionId)"
goal_response="$(post_goal merge.propose \
  "{\"sessionId\":\"$goal_child_session_2\",\"candidate\":{\"contributionIds\":[\"$goal_contribution_1\",\"$goal_contribution_2\"],\"contractVersionId\":\"$goal_child_contract_id\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"退回路径通过\",\"幂等重放通过\"],\"risks\":[],\"selfCheck\":\"逐条检查目标契约，已补齐下一 Session 证据\"}}" \
  "$(new_uuid)")"
goal_gate_2="$(json_field "$goal_response" result.reviewGateId)"
post_goal review.ai_record \
  "{\"reviewGateId\":\"$goal_gate_2\",\"reviewerIdentity\":\"reviewer-b\",\"decision\":\"recommend_accept\",\"rationale\":\"退回要求已经满足\",\"contractCheck\":{\"passed\":true},\"retestEvidence\":[\"重跑 HTTP 流程\"]}" \
  "$(new_uuid)" >/dev/null
post_goal review.human_decide \
  "{\"reviewGateId\":\"$goal_gate_2\",\"decision\":\"accept\",\"rationale\":\"接受子目标完整贡献\",\"selectedContributionIds\":[\"$goal_contribution_1\",\"$goal_contribution_2\"]}" \
  "$(new_uuid)" >/dev/null

post_goal session.resume \
  "{\"sessionId\":\"$goal_root_session_id\",\"resolution\":\"子目标已接受，回到父目标做整合验证\"}" \
  "$(new_uuid)" >/dev/null
goal_response="$(post_goal session.add_contribution \
  "{\"sessionId\":\"$goal_root_session_id\",\"kind\":\"evidence\",\"title\":\"父目标整合验证\",\"body\":\"父 Session 显式恢复并核验子目标回流\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}" \
  "$(new_uuid)")"
goal_root_contribution="$(json_field "$goal_response" result.contributionId)"
goal_response="$(post_goal merge.propose \
  "{\"sessionId\":\"$goal_root_session_id\",\"candidate\":{\"contributionIds\":[\"$goal_root_contribution\"],\"contractVersionId\":\"$goal_root_contract_id\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"父目标整合验证通过\"],\"risks\":[],\"selfCheck\":\"子目标已接受且父目标已整合验证\"}}" \
  "$(new_uuid)")"
goal_root_gate="$(json_field "$goal_response" result.reviewGateId)"
post_goal review.ai_record \
  "{\"reviewGateId\":\"$goal_root_gate\",\"reviewerIdentity\":\"reviewer-root\",\"decision\":\"recommend_accept\",\"rationale\":\"根契约证据完整\",\"contractCheck\":{\"passed\":true},\"retestEvidence\":[\"完整快照核验\"]}" \
  "$(new_uuid)" >/dev/null
post_goal review.human_decide \
  "{\"reviewGateId\":\"$goal_root_gate\",\"decision\":\"accept\",\"rationale\":\"确认根目标完成\",\"selectedContributionIds\":[\"$goal_root_contribution\"]}" \
  "$(new_uuid)" >/dev/null

goal_snapshot="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1])
root=sys.argv[2]; child=sys.argv[3]
branches={b["id"]:b for b in s["branches"]}
sessions=sorted((x for x in s["sessions"] if x["goalBranchId"]==child), key=lambda x:x["sessionNumber"])
assert s["modelVersion"] == "goal-branch-v1"
assert s["project"]["state"] == "completed"
assert branches[root]["status"] == "completed"
assert branches[child]["status"] == "integrated"
assert [x["status"] for x in sessions] == ["review_rejected", "accepted"]
assert len(s["integrations"]) == 2
assert all(i["gitIntegrationStatus"] == "not_attempted" for i in s["integrations"])
assert not [a for a in s["attentionItems"] if a["status"] == "open"]' \
  "$goal_snapshot" "$goal_root_branch_id" "$goal_child_branch_id"

goal_form_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证 HTML 表单共享目标枝干服务"}' \
  "$goal_http_base/api/projects")"
goal_form_project_id="$(json_field "$goal_form_project_response" id)"
goal_form_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  --data-urlencode action=proposal.create \
  --data-urlencode "payload={\"revision\":$goal_root_revision}" \
  "$goal_http_base/projects/$goal_form_project_id/goal-commands")"
[[ "$goal_form_status" == 303 ]]
goal_form_snapshot="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_form_project_id/goal-graph")"
python3 -c 'import json,sys; assert len(json.loads(sys.argv[1])["proposals"]) == 1' \
  "$goal_form_snapshot"

echo "goal HTTP flow passed: Proposal -> child branch -> reject -> next Session -> accept -> root complete"
