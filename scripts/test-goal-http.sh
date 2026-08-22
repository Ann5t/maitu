#!/usr/bin/env bash
set -euo pipefail

goal_http_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
goal_http_suffix="$$"
goal_http_network="fudian-goal-http-test-$goal_http_suffix"
goal_http_db="fudian-goal-http-db-$goal_http_suffix"
goal_http_app="fudian-goal-http-app-$goal_http_suffix"
goal_http_tmp="$(mktemp -d)"
goal_http_worker_bootstrap="goal_http_worker_bootstrap_0123456789abcdef"

# shellcheck source=scripts/review-worker-test-lib.sh
. "$goal_http_repo_root/scripts/review-worker-test-lib.sh"

cleanup_goal_http() {
  local goal_http_status="$?"
  if [[ "$goal_http_status" -ne 0 ]] && docker inspect "$goal_http_app" >/dev/null 2>&1; then
    docker logs "$goal_http_app" >&2 || true
  fi
  if [[ "$goal_http_app" == fudian-goal-http-app-* ]]; then
    docker rm -f "$goal_http_app" >/dev/null 2>&1 || true
  fi
  if [[ "$goal_http_db" == fudian-goal-http-db-* ]]; then
    docker rm -f "$goal_http_db" >/dev/null 2>&1 || true
  fi
  if [[ "$goal_http_network" == fudian-goal-http-test-* ]]; then
    docker network rm "$goal_http_network" >/dev/null 2>&1 || true
  fi
  [[ "$goal_http_tmp" == /tmp/tmp.* ]] && rm -rf "$goal_http_tmp"
  return "$goal_http_status"
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
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$goal_http_worker_bootstrap" \
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
test_review_gate "$goal_http_base" "$goal_http_worker_bootstrap" "$goal_project_id" \
  "$goal_gate_1" recommend_reject "缺少退回后的继续证据"
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
test_review_gate "$goal_http_base" "$goal_http_worker_bootstrap" "$goal_project_id" \
  "$goal_gate_2" recommend_accept "退回要求已经满足"
goal_response="$(post_goal review.human_decide \
  "{\"reviewGateId\":\"$goal_gate_2\",\"decision\":\"accept\",\"rationale\":\"接受子目标完整贡献\",\"selectedContributionIds\":[\"$goal_contribution_1\",\"$goal_contribution_2\"]}" \
  "$(new_uuid)")"
goal_integration_id="$(json_field "$goal_response" result.integrationId)"
test_integrate_goal "$goal_http_base" "$goal_http_worker_bootstrap" "$goal_project_id" \
  "$goal_integration_id" >/dev/null
goal_response="$(post_goal session.add_contribution \
  "{\"sessionId\":\"$goal_root_session_id\",\"kind\":\"evidence\",\"title\":\"父目标整合验证\",\"body\":\"父 Session 显式恢复并核验子目标回流\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}" \
  "$(new_uuid)")"
goal_root_contribution="$(json_field "$goal_response" result.contributionId)"
goal_response="$(post_goal merge.propose \
  "{\"sessionId\":\"$goal_root_session_id\",\"candidate\":{\"contributionIds\":[\"$goal_root_contribution\"],\"contractVersionId\":\"$goal_root_contract_id\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"父目标整合验证通过\"],\"risks\":[],\"selfCheck\":\"子目标已接受且父目标已整合验证\"}}" \
  "$(new_uuid)")"
goal_root_gate="$(json_field "$goal_response" result.reviewGateId)"
test_review_gate "$goal_http_base" "$goal_http_worker_bootstrap" "$goal_project_id" \
  "$goal_root_gate" recommend_accept "根契约证据完整"
post_goal review.human_decide \
  "{\"reviewGateId\":\"$goal_root_gate\",\"decision\":\"accept\",\"rationale\":\"确认根目标完成\",\"selectedContributionIds\":[\"$goal_root_contribution\"]}" \
  "$(new_uuid)" >/dev/null

goal_snapshot="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1])
root=sys.argv[2]; child=sys.argv[3]
branches={b["id"]:b for b in s["branches"]}
sessions=sorted((x for x in s["sessions"] if x["goalBranchId"]==child), key=lambda x:x["sessionNumber"])
assert s["modelVersion"] == "goal-branch-v2"
assert s["project"]["state"] == "completed"
assert branches[root]["status"] == "completed"
assert branches[child]["status"] == "integrated"
assert [x["status"] for x in sessions] == ["review_rejected", "accepted"]
assert len(s["integrations"]) == 1
assert s["integrations"][0]["gitIntegrationStatus"] == "applied"
assert not [a for a in s["attentionItems"] if a["status"] == "open"]' \
  "$goal_snapshot" "$goal_root_branch_id" "$goal_child_branch_id"

# A second isolated project covers contract evolution, first-class Evidence, candidate withdrawal,
# explicit stop and archival without making the already dense acceptance flow harder to inspect.
goal_v2_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"验证契约演进、Evidence、撤回和停止"}' \
  "$goal_http_base/api/projects")"
goal_project_id="$(json_field "$goal_v2_project_response" id)"
goal_response="$(post_goal proposal.create \
  "{\"revision\":$goal_root_revision}" "$(new_uuid)")"
goal_v2_proposal_id="$(json_field "$goal_response" result.proposalId)"
post_goal proposal.submit \
  "{\"proposalId\":\"$goal_v2_proposal_id\",\"expectedRevision\":1}" \
  "$(new_uuid)" >/dev/null
goal_response="$(post_goal proposal.approve \
  "{\"proposalId\":\"$goal_v2_proposal_id\",\"expectedRevision\":1,\"branchName\":\"契约演进根目标\",\"assignment\":\"验证 v2 状态机\",\"agentIdentity\":\"worker-v2\"}" \
  "$(new_uuid)")"
goal_v2_branch_id="$(json_field "$goal_response" result.goalBranchId)"
goal_v2_session_1="$(json_field "$goal_response" result.sessionId)"
goal_v2_contract_1="$(json_field "$goal_response" result.contractVersionId)"
goal_v2_contract_base="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_project_id/goal-graph" | \
  python3 -c 'import json,sys
s=json.load(sys.stdin); target=sys.argv[1]
c=next(x for x in s["contracts"] if x["id"]==target)
keys=["desiredOutcome","hardConstraints","subjectivePreferences","unknowns","nonGoals","validationPlan","judgmentTriggers","stopConditions","expectedContributions"]
contract={key:c[key] for key in keys}
contract["exploration"]=c["explorationPolicy"]
print(json.dumps(contract,ensure_ascii=False))' "$goal_v2_contract_1")"

goal_contract_rejected="$(python3 -c 'import json,sys
value=json.loads(sys.argv[1])
value["hardConstraints"].append("这条候选将被拒绝")
print(json.dumps(value,ensure_ascii=False))' "$goal_v2_contract_base")"
goal_response="$(post_goal contract.propose_revision \
  "{\"goalBranchId\":\"$goal_v2_branch_id\",\"expectedContractVersionId\":\"$goal_v2_contract_1\",\"proposedBySessionId\":\"$goal_v2_session_1\",\"contract\":$goal_contract_rejected,\"reason\":\"验证拒绝不改变当前契约\",\"sourceAnnotations\":[{\"fieldPath\":\"/hardConstraints\",\"sourceKind\":\"agent_inference\",\"sourceRef\":null,\"note\":\"测试生成的候选约束\"}]}" \
  "$(new_uuid)")"
goal_v2_contract_request_rejected="$(json_field "$goal_response" result.revisionRequestId)"
post_goal contract.reject_revision \
  "{\"revisionRequestId\":\"$goal_v2_contract_request_rejected\",\"rationale\":\"这项约束不符合目标，只保留审计记录\"}" \
  "$(new_uuid)" >/dev/null

goal_contract_accepted="$(python3 -c 'import json,sys
value=json.loads(sys.argv[1])
value["unknowns"].append("撤回候选后是否保持同一枝干")
value["validationPlan"].append("冻结结构化 Evidence 并验证撤回")
value["exploration"]={
  "mode":"exploration",
  "budgets":["最多验证两个候选后请求判断"],
  "candidateOutputs":["可撤回并保留历史的候选现场"],
  "uncertaintyReduction":["能排除候选被静默修改或冒充完成"]
}
print(json.dumps(value,ensure_ascii=False))' "$goal_v2_contract_base")"
goal_response="$(post_goal contract.propose_revision \
  "{\"goalBranchId\":\"$goal_v2_branch_id\",\"expectedContractVersionId\":\"$goal_v2_contract_1\",\"proposedBySessionId\":\"$goal_v2_session_1\",\"contract\":$goal_contract_accepted,\"reason\":\"把候选撤回纳入有边界的探索验证\",\"sourceAnnotations\":[{\"fieldPath\":\"/unknowns\",\"sourceKind\":\"human_input\",\"sourceRef\":null,\"note\":\"用户要求不能遗漏撤回路径\"},{\"fieldPath\":\"/validationPlan\",\"sourceKind\":\"evidence\",\"sourceRef\":\"planned:http-v2-flow\",\"note\":\"将由隔离 HTTP 流程核验\"},{\"fieldPath\":\"/exploration\",\"sourceKind\":\"human_input\",\"sourceRef\":null,\"note\":\"未知效果用候选、预算和判断边界推进\"}]}" \
  "$(new_uuid)")"
goal_v2_contract_request_accepted="$(json_field "$goal_response" result.revisionRequestId)"
goal_v2_contract_3="$(json_field "$goal_response" result.proposedContractVersionId)"
goal_response="$(post_goal contract.accept_revision \
  "{\"revisionRequestId\":\"$goal_v2_contract_request_accepted\",\"rationale\":\"差异和来源清楚，接受为活动契约\"}" \
  "$(new_uuid)")"
[[ "$(json_field "$goal_response" result.pausedSessionId)" == "$goal_v2_session_1" ]]
post_goal session.resume \
  "{\"sessionId\":\"$goal_v2_session_1\",\"resolution\":\"已阅读契约差异，在 v3 下继续\"}" \
  "$(new_uuid)" >/dev/null

goal_response="$(post_goal session.add_evidence \
  "{\"sessionId\":\"$goal_v2_session_1\",\"kind\":\"test\",\"stance\":\"supports\",\"claim\":\"候选撤回保持旧候选冻结\",\"observation\":\"隔离 HTTP 将 Gate 转为 withdrawn 并要求下一 Session\",\"sourceUri\":null,\"artifactId\":null,\"toolCallId\":null,\"verificationStatus\":\"verified\"}" \
  "$(new_uuid)")"
goal_v2_evidence="$(json_field "$goal_response" result.evidenceId)"
goal_response="$(post_goal session.add_contribution \
  "{\"sessionId\":\"$goal_v2_session_1\",\"kind\":\"finding\",\"title\":\"撤回前候选\",\"body\":\"这项结果随后因新证据撤回\",\"artifactId\":null,\"evidenceRefs\":[],\"evidenceIds\":[\"$goal_v2_evidence\"],\"supersedesId\":null}" \
  "$(new_uuid)")"
goal_v2_contribution="$(json_field "$goal_response" result.contributionId)"
goal_response="$(post_goal merge.propose \
  "{\"sessionId\":\"$goal_v2_session_1\",\"candidate\":{\"contributionIds\":[\"$goal_v2_contribution\"],\"evidenceIds\":[\"$goal_v2_evidence\"],\"contractVersionId\":\"$goal_v2_contract_3\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"候选已冻结\"],\"risks\":[],\"selfCheck\":\"先冻结，再用新证据撤回\"}}" \
  "$(new_uuid)")"
goal_v2_gate="$(json_field "$goal_response" result.reviewGateId)"

goal_frozen_body="$(python3 -c 'import json,sys,uuid; print(json.dumps({
  "clientRequestId":str(uuid.uuid4()),"action":"session.add_evidence","payload":{
    "sessionId":sys.argv[1],"kind":"test","stance":"supports","claim":"非法写入",
    "observation":"冻结后不应出现","sourceUri":None,"artifactId":None,"toolCallId":None,
    "verificationStatus":"verified"}}))' "$goal_v2_session_1")"
goal_frozen_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$goal_frozen_body" \
  "$goal_http_base/api/v1/projects/$goal_project_id/goal-commands")"
[[ "$goal_frozen_status" == 409 ]]

post_goal merge.withdraw \
  "{\"reviewGateId\":\"$goal_v2_gate\",\"reason\":\"发现候选没有覆盖停止语义\",\"newEvidence\":[\"冻结后的反例不写入旧候选\"]}" \
  "$(new_uuid)" >/dev/null

goal_parallel_payload="{\"goalBranchId\":\"$goal_v2_branch_id\",\"previousSessionId\":\"$goal_v2_session_1\",\"assignment\":\"根据撤回原因决定安全停止\",\"agentIdentity\":\"worker-v2\"}"
for goal_parallel_index in 1 2; do
  goal_parallel_body="$(python3 -c 'import json,sys; print(json.dumps({
    "clientRequestId":sys.argv[1],"action":"session.start_next","payload":json.loads(sys.argv[2])
  },ensure_ascii=False))' "$(new_uuid)" "$goal_parallel_payload")"
  curl -sS -w $'\n%{http_code}' -H 'content-type: application/json' \
    -d "$goal_parallel_body" \
    "$goal_http_base/api/v1/projects/$goal_project_id/goal-commands" \
    > "$goal_http_tmp/parallel-$goal_parallel_index" &
done
wait
goal_parallel_statuses="$(for goal_parallel_index in 1 2; do
  printf '%s\n' "$(tail -n1 "$goal_http_tmp/parallel-$goal_parallel_index")"
done | sort | paste -sd: -)"
[[ "$goal_parallel_statuses" == "200:409" ]]
goal_parallel_success_file="$(for goal_parallel_index in 1 2; do
  if [[ "$(tail -n1 "$goal_http_tmp/parallel-$goal_parallel_index")" == 200 ]]; then
    printf '%s' "$goal_http_tmp/parallel-$goal_parallel_index"
  fi
done)"
goal_response="$(sed '$d' "$goal_parallel_success_file")"
goal_v2_session_2="$(json_field "$goal_response" result.sessionId)"
post_goal session.stop \
  "{\"sessionId\":\"$goal_v2_session_2\",\"reason\":\"验证明确停止保留负面结论且不冒充完成\"}" \
  "$(new_uuid)" >/dev/null
post_goal goal_branch.archive \
  "{\"goalBranchId\":\"$goal_v2_branch_id\",\"reason\":\"终态验证完成后归档\"}" \
  "$(new_uuid)" >/dev/null

goal_v2_snapshot="$(curl -fsS "$goal_http_base/api/v1/projects/$goal_project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); branch=sys.argv[2]; contract=sys.argv[3]
b=next(x for x in s["branches"] if x["id"]==branch)
sessions=sorted(s["sessions"],key=lambda x:x["sessionNumber"])
assert s["modelVersion"]=="goal-branch-v2"
assert s["project"]["state"]=="stopped"
assert b["status"]=="archived" and b["archivedFromStatus"]=="stopped"
assert b["currentContractVersionId"]==contract
active_contract=next(x for x in s["contracts"] if x["id"]==contract)
assert active_contract["explorationPolicy"]["mode"]=="exploration"
assert active_contract["explorationPolicy"]["budgets"]==["最多验证两个候选后请求判断"]
assert [x["status"] for x in sessions]==["review_rejected","stopped"]
assert [x["status"] for x in s["contractRevisionRequests"]]==["rejected","accepted"]
assert len(s["contractRevisionDecisions"])==2
assert len(s["contractProvenance"])>=5
assert len(s["evidence"])==1 and s["evidence"][0]["verificationStatus"]=="verified"
assert len(s["contributionEvidence"])==1
assert len(s["reviewGateEvidence"])==1
assert s["reviewGates"][0]["status"]=="withdrawn"
assert not [a for a in s["attentionItems"] if a["status"]=="open"]' \
  "$goal_v2_snapshot" "$goal_v2_branch_id" "$goal_v2_contract_3"

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

echo "goal HTTP flow passed: Proposal, contract diff/decision, Evidence, withdraw, stop/archive and root completion"
