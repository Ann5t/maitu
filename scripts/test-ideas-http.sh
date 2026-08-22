#!/usr/bin/env bash
set -euo pipefail

idea_http_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
idea_http_suffix="$$"
idea_http_network="fudian-idea-http-test-$idea_http_suffix"
idea_http_db="fudian-idea-http-db-$idea_http_suffix"
idea_http_app="fudian-idea-http-app-$idea_http_suffix"
idea_http_artifacts="$(mktemp -d)"

cleanup_idea_http() {
  if [[ "$idea_http_app" == fudian-idea-http-app-* ]]; then
    docker rm -f "$idea_http_app" >/dev/null 2>&1 || true
  fi
  if [[ "$idea_http_db" == fudian-idea-http-db-* ]]; then
    docker rm -f "$idea_http_db" >/dev/null 2>&1 || true
  fi
  if [[ "$idea_http_network" == fudian-idea-http-test-* ]]; then
    docker network rm "$idea_http_network" >/dev/null 2>&1 || true
  fi
  [[ "$idea_http_artifacts" == /tmp/tmp.* ]] && rm -rf "$idea_http_artifacts"
}
trap cleanup_idea_http EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys
value=json.loads(sys.argv[1])
for key in sys.argv[2].split("."):
    value=value[key]
print(str(value).lower() if isinstance(value,bool) else value)' "$1" "$2"
}

post_command() {
  local command_url="$1"
  local command_action="$2"
  local command_payload="$3"
  local command_request_id="$4"
  local command_body
  command_body="$(python3 -c 'import json,sys
print(json.dumps({
  "clientRequestId":sys.argv[1],
  "action":sys.argv[2],
  "payload":json.loads(sys.argv[3])
},ensure_ascii=False))' "$command_request_id" "$command_action" "$command_payload")"
  curl -fsS -H 'content-type: application/json' -d "$command_body" "$command_url"
}

docker network create "$idea_http_network" >/dev/null
docker run -d --name "$idea_http_db" --network "$idea_http_network" \
  --network-alias idea-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for idea_http_attempt in $(seq 1 30); do
  if docker exec "$idea_http_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  if [[ "$idea_http_attempt" == 30 ]]; then
    docker logs "$idea_http_db"
    exit 1
  fi
  sleep 1
done

docker run -d --name "$idea_http_app" --network "$idea_http_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@idea-db:5432/fudian_test \
  -e ARTIFACT_ROOT=/tmp/fudian-test-artifacts \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  --mount "type=bind,src=$idea_http_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null
idea_http_port="$(docker port "$idea_http_app" 3000/tcp | sed 's/.*://')"
idea_http_base="http://127.0.0.1:$idea_http_port"

for idea_http_attempt in $(seq 1 45); do
  if curl -fsS "$idea_http_base/api/health" >/dev/null 2>&1; then
    break
  fi
  if [[ "$idea_http_attempt" == 45 ]]; then
    docker logs "$idea_http_app"
    exit 1
  fi
  sleep 1
done

idea_create_request="$(new_uuid)"
idea_response="$(post_command "$idea_http_base/api/v1/ideas" idea.create \
  '{"revision":{"title":"","body":"把科研目标展开成可审查的独立枝干","sourceKind":"text","sourceRef":null,"revisionReason":null}}' \
  "$idea_create_request")"
idea_one="$(json_field "$idea_response" result.ideaId)"
[[ "$(json_field "$idea_response" replayed)" == false ]]
idea_replay="$(post_command "$idea_http_base/api/v1/ideas" idea.create \
  '{"revision":{"title":"","body":"把科研目标展开成可审查的独立枝干","sourceKind":"text","sourceRef":null,"revisionReason":null}}' \
  "$idea_create_request")"
[[ "$(json_field "$idea_replay" replayed)" == true ]]

idea_conflict_body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"action":"idea.create","payload":{
"revision":{"title":"不同输入","body":"重复幂等键不能改变输入","sourceKind":"text","sourceRef":None,"revisionReason":None}}}))' \
  "$idea_create_request")"
idea_conflict_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$idea_conflict_body" \
  "$idea_http_base/api/v1/ideas")"
[[ "$idea_conflict_status" == 409 ]]

post_command "$idea_http_base/api/v1/ideas/$idea_one/commands" idea.revise \
  '{"expectedRevision":1,"revision":{"title":"科研目标枝干","body":"把科研目标展开成可审查的独立枝干，并保留未知与反例。","sourceKind":"text","sourceRef":null,"revisionReason":"补充未知与反例"}}' \
  "$(new_uuid)" >/dev/null

idea_response="$(post_command "$idea_http_base/api/v1/ideas" idea.create \
  '{"revision":{"title":"移动工作现场","body":"在手机和平板上也能查看文件、浏览器与测试现场","sourceKind":"text","sourceRef":null,"revisionReason":null}}' \
  "$(new_uuid)")"
idea_two="$(json_field "$idea_response" result.ideaId)"

idea_stale_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"action":"idea.link","payload":{
"expectedSourceRevision":1,"targetIdeaId":sys.argv[2],"expectedTargetRevision":1,
"relation":"supports","rationale":"移动现场支持目标管理"}}))' "$(new_uuid)" "$idea_two")" \
  "$idea_http_base/api/v1/ideas/$idea_one/commands")"
[[ "$idea_stale_status" == 409 ]]
post_command "$idea_http_base/api/v1/ideas/$idea_one/commands" idea.link \
  "{\"expectedSourceRevision\":2,\"targetIdeaId\":\"$idea_two\",\"expectedTargetRevision\":1,\"relation\":\"supports\",\"rationale\":\"移动现场支持目标管理\"}" \
  "$(new_uuid)" >/dev/null

proposal_revision="$(python3 -c 'import json,sys
print(json.dumps({
  "title":"目标枝干科研工作台",
  "projectIntent":"开发可在多设备使用、以目标枝干推进科研的软件",
  "whyNow":"核心语义已经可以形成可审查实现",
  "rootGoal":{
    "whyNeeded":"需要先建立可运行且可验证的根目标",
    "contract":{
      "desiredOutcome":"完成可运行的目标枝干科研工作台",
      "hardConstraints":["未经用户批准不合并"],
      "subjectivePreferences":["文字清楚可读"],
      "unknowns":["最终图布局手感"],
      "nonGoals":["不以文档代替实现"],
      "validationPlan":["运行隔离 HTTP 与浏览器测试"],
      "judgmentTriggers":["形成多设备候选后请用户判断"],
      "stopConditions":["用户接受或明确停止"],
      "expectedContributions":["代码","证据"]
    },
    "expectedContributions":["代码","证据"],
    "explorationPlan":["先验证关键风险"],
    "contextInheritance":{},
    "toolRequirements":["rust","playwright"],
    "inferences":[],
    "revisionReason":None
  },
  "retainedNotes":["保留目标枝干语义"],
  "omittedNotes":["暂不固定最终布局"],
  "sources":[
    {"ideaId":sys.argv[1],"ideaRevision":2,"role":"source","rationale":"核心目标语义"},
    {"ideaId":sys.argv[2],"ideaRevision":1,"role":"supporting","rationale":"多设备工作现场"}
  ],
  "revisionReason":None
},ensure_ascii=False))' "$idea_one" "$idea_two")"
proposal_response="$(post_command "$idea_http_base/api/v1/ideas/$idea_one/commands" \
  project_proposal.create "{\"revision\":$proposal_revision}" "$(new_uuid)")"
proposal_id="$(json_field "$proposal_response" result.projectProposalId)"

post_command "$idea_http_base/api/v1/project-proposals/$proposal_id/commands" \
  project_proposal.submit '{"expectedRevision":1}' "$(new_uuid)" >/dev/null
project_count_before="$(docker exec "$idea_http_db" psql -U fudian_test -d fudian_test -Atc \
  'SELECT count(*) FROM projects')"
[[ "$project_count_before" == 0 ]]

approve_request="$(new_uuid)"
approve_response="$(post_command "$idea_http_base/api/v1/project-proposals/$proposal_id/commands" \
  project_proposal.approve '{"expectedRevision":1}' "$approve_request")"
project_id="$(json_field "$approve_response" result.projectId)"
root_goal_proposal="$(json_field "$approve_response" result.rootGoalProposalId)"
approve_replay="$(post_command "$idea_http_base/api/v1/project-proposals/$proposal_id/commands" \
  project_proposal.approve '{"expectedRevision":1}' "$approve_request")"
[[ "$(json_field "$approve_replay" replayed)" == true ]]

origin_fingerprint="$(docker exec "$idea_http_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT p.status || ':' || gp.status || ':' || count(poi.*) || ':' ||
          string_agg(poi.idea_revision::text, ',' ORDER BY poi.idea_revision DESC)
   FROM project_proposals p
   JOIN project_origins po ON po.project_proposal_id = p.id
   JOIN goal_branch_proposals gp ON gp.id = po.root_goal_proposal_id
   JOIN project_origin_ideas poi ON poi.project_id = po.project_id
   WHERE p.id = '$proposal_id' AND po.project_id = '$project_id'
     AND gp.id = '$root_goal_proposal'
   GROUP BY p.status, gp.status")"
[[ "$origin_fingerprint" == "approved:draft:2:2,1" ]]

idea_snapshot="$(curl -fsS "$idea_http_base/api/v1/ideas/$idea_one")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1])
assert s["idea"]["state"] == "promoted"
assert s["idea"]["currentRevision"] == 2
assert len(s["links"]) == 1
assert s["proposals"][0]["status"] == "approved"
assert s["proposalRevisions"][0]["omittedNotes"] == ["暂不固定最终布局"]' \
  "$idea_snapshot"
goal_snapshot="$(curl -fsS "$idea_http_base/api/v1/projects/$project_id/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1])
assert len(s["proposals"]) == 1
assert s["proposals"][0]["status"] == "draft"
assert len(s["branches"]) == 0' "$goal_snapshot"

curl -fsS "$idea_http_base/ideas?view=map" | grep -q '关系图'
curl -fsS "$idea_http_base/ideas/$idea_one" | grep -q 'ProjectProposal'
curl -fsS "$idea_http_base/projects/$project_id?tab=goals" | grep -q '目标枝干工作台'

echo "idea → ProjectProposal → Project HTTP flow passed with immutable provenance"
