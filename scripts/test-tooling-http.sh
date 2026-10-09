#!/usr/bin/env bash
set -euo pipefail

tooling_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tooling_suffix="$$"
tooling_network="fudian-tooling-test-$tooling_suffix"
tooling_db="fudian-tooling-db-$tooling_suffix"
tooling_app="fudian-tooling-app-$tooling_suffix"

cleanup_tooling_stack() {
  [[ "$tooling_app" == fudian-tooling-app-* ]] \
    && docker rm -fv "$tooling_app" >/dev/null 2>&1 || true
  [[ "$tooling_db" == fudian-tooling-db-* ]] \
    && docker rm -fv "$tooling_db" >/dev/null 2>&1 || true
  [[ "$tooling_network" == fudian-tooling-test-* ]] \
    && docker network rm "$tooling_network" >/dev/null 2>&1 || true
}
trap cleanup_tooling_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys; value=json.loads(sys.argv[1]);
for key in sys.argv[2].split("."):
    value=value[key]
print(str(value).lower() if isinstance(value,bool) else value)' "$1" "$2"
}

post_goal_for() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  local request_id="$(new_uuid)"
  local body
  body="$(python3 -c 'import json,sys; print(json.dumps({
    "clientRequestId":sys.argv[1],"action":sys.argv[2],"payload":json.loads(sys.argv[3])
  },ensure_ascii=False))' "$request_id" "$action" "$payload")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$tooling_base/api/v1/projects/$project_id/goal-commands"
}

create_running_session() {
  local intent="$1"
  local agent_identity="$2"
  local project_response
  local project_id
  local proposal_response
  local proposal_id
  local approval_response
  project_response="$(curl -fsS -H 'content-type: application/json' \
    -d "{\"intent\":\"$intent\"}" "$tooling_base/api/projects")"
  project_id="$(json_field "$project_response" id)"
  proposal_response="$(post_goal_for "$project_id" proposal.create \
    "{\"revision\":$tooling_goal_revision}")"
  proposal_id="$(json_field "$proposal_response" result.proposalId)"
  post_goal_for "$project_id" proposal.submit \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1}" >/dev/null
  approval_response="$(post_goal_for "$project_id" proposal.approve \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1,\"branchName\":\"工具环境目标\",\"assignment\":\"验证固定插件环境\",\"agentIdentity\":\"$agent_identity\"}")"
  printf '%s|%s|%s|%s' \
    "$project_id" \
    "$(json_field "$approval_response" result.goalBranchId)" \
    "$(json_field "$approval_response" result.sessionId)" \
    "$(json_field "$approval_response" result.contractVersionId)"
}

docker network create "$tooling_network" >/dev/null
docker run -d --name "$tooling_db" --network "$tooling_network" \
  --network-alias tooling-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for tooling_attempt in $(seq 1 30); do
  if docker exec "$tooling_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$tooling_attempt" == 30 ]] && docker logs "$tooling_db" && exit 1
  sleep 1
done

docker run -d --name "$tooling_app" --network "$tooling_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@tooling-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=disabled \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/tmp/fudian-tooling-artifacts \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$tooling_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null

tooling_port="$(docker port "$tooling_app" 3000/tcp | sed -n 's/.*://p')"
tooling_base="http://127.0.0.1:$tooling_port"
for tooling_attempt in $(seq 1 60); do
  if curl -fsS "$tooling_base/api/health" >/dev/null 2>&1; then
    break
  fi
  [[ "$tooling_attempt" == 60 ]] && docker logs "$tooling_app" && exit 1
  sleep 1
done

tooling_catalog="$(curl -fsS "$tooling_base/api/v1/plugins")"
python3 -c 'import json,sys
p=json.loads(sys.argv[1])["plugins"]
assert [(x["plugin"]["pluginId"],x["plugin"]["version"]) for x in p] == [("fudian.tools.reference","1.0.0")]' \
  "$tooling_catalog"
tooling_plugin_1="$(curl -fsS \
  "$tooling_base/api/v1/plugins/fudian.tools.reference/1.0.0")"
tooling_plugin_ref_1="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); print(json.dumps({k:p[k] for k in ("pluginId","version","contentDigest")}))' \
  "$tooling_plugin_1")"

tooling_plugin_draft_2="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p.pop("contentDigest"); p["version"]="2.0.0"; p["displayName"]="Fudian reference tools v2"; print(json.dumps(p))' \
  "$tooling_plugin_1")"
tooling_plugin_2="$(curl -fsS -H 'content-type: application/json' \
  -d "$tooling_plugin_draft_2" "$tooling_base/api/v1/plugins")"
tooling_plugin_ref_2="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); print(json.dumps({k:p[k] for k in ("pluginId","version","contentDigest")}))' \
  "$tooling_plugin_2")"
tooling_latest_2="$(curl -fsS -H 'content-type: application/json' \
  -d '{"pluginId":"fudian.tools.reference","version":"latest"}' \
  "$tooling_base/api/v1/plugins/resolve")"
[[ "$(json_field "$tooling_latest_2" version)" == 2.0.0 ]]

tooling_goal_revision='{
  "whyNeeded":"验证不同 Session 的固定插件环境",
  "contract":{
    "desiredOutcome":"Session 使用可复现且互相隔离的工具环境",
    "hardConstraints":["不修改全局插件包"],
    "subjectivePreferences":[],
    "unknowns":[],
    "nonGoals":["不启动外部 Runner"],
    "validationPlan":["调用参考 Mock Tool Broker"],
    "judgmentTriggers":["环境重绑定时拒绝"],
    "stopConditions":["调用结果绑定准确指纹"],
    "expectedContributions":["工具审计记录"]
  },
  "expectedContributions":[],
  "explorationPlan":[],
  "contextInheritance":{},
  "toolRequirements":["fudian.tools.reference"],
  "inferences":[],
  "revisionReason":null
}'

IFS='|' read -r tooling_project_1 tooling_branch_1 tooling_session_1 tooling_contract_1 \
  <<< "$(create_running_session '验证参考插件 1.0.0 环境' 'tool-worker-1')"
IFS='|' read -r tooling_project_2 tooling_branch_2 tooling_session_2 tooling_contract_2 \
  <<< "$(create_running_session '验证参考插件 2.0.0 环境' 'tool-worker-2')"

tooling_environment_1_payload="$(python3 -c 'import json,sys
plugin=json.loads(sys.argv[1]); print(json.dumps({
 "schemaVersion":1,"baseRuntime":{"kind":"mock","digest":"sha256:"+"b"*64},
 "plugins":[plugin],"toolchains":{"python":"3.12.0"},
 "dependencyLocks":[{"path":"requirements.lock","sha256":"a"*64}],
 "targetPlatform":"x86_64-unknown-linux-gnu","features":[],"buildParameters":{},
 "networkPolicy":"denied","resourcePolicy":{"cpuMillis":1000,"memoryMiB":256,"diskMiB":256,"timeoutSeconds":30},
 "environmentPolicy":{"allowedNames":["CI"],"secretReferences":[]}
}))' "$tooling_plugin_ref_1")"
tooling_environment_2_payload="$(python3 -c 'import json,sys
plugin=json.loads(sys.argv[1]); print(json.dumps({
 "schemaVersion":1,"baseRuntime":{"kind":"mock","digest":"sha256:"+"b"*64},
 "plugins":[plugin],"toolchains":{"python":"2.7.18"},
 "dependencyLocks":[{"path":"requirements.lock","sha256":"c"*64}],
 "targetPlatform":"x86_64-unknown-linux-gnu","features":[],"buildParameters":{},
 "networkPolicy":"denied","resourcePolicy":{"cpuMillis":1000,"memoryMiB":256,"diskMiB":256,"timeoutSeconds":30},
 "environmentPolicy":{"allowedNames":["CI"],"secretReferences":[]}
}))' "$tooling_plugin_ref_2")"
tooling_environment_1="$(curl -fsS -H 'content-type: application/json' \
  -d "$tooling_environment_1_payload" "$tooling_base/api/v1/environments")"
tooling_environment_2="$(curl -fsS -H 'content-type: application/json' \
  -d "$tooling_environment_2_payload" "$tooling_base/api/v1/environments")"
tooling_environment_id_1="$(json_field "$tooling_environment_1" id)"
tooling_environment_id_2="$(json_field "$tooling_environment_2" id)"
tooling_fingerprint_1="$(json_field "$tooling_environment_1" fingerprint)"
tooling_fingerprint_2="$(json_field "$tooling_environment_2" fingerprint)"
[[ "$tooling_fingerprint_1" != "$tooling_fingerprint_2" ]]

tooling_bind_1="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"environmentManifestId\":\"$tooling_environment_id_1\"}" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/environment")"
[[ "$(json_field "$tooling_bind_1" environmentFingerprint)" == "$tooling_fingerprint_1" ]]
tooling_bind_replay="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"environmentManifestId\":\"$tooling_environment_id_1\"}" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/environment")"
[[ "$(json_field "$tooling_bind_replay" replayed)" == true ]]
tooling_rebind_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"environmentManifestId\":\"$tooling_environment_id_2\"}" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/environment")"
[[ "$tooling_rebind_status" == 409 ]]

curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"environmentManifestId\":\"$tooling_environment_id_2\"}" \
  "$tooling_base/api/v1/projects/$tooling_project_2/sessions/$tooling_session_2/environment" >/dev/null

tooling_call_request_id_1="$(new_uuid)"
tooling_call_payload_1="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"plugin":json.loads(sys.argv[2]),
 "toolName":"inspect","input":{"library":"v1"},"baseWorkspaceSnapshot":"sha256:"+"e"*64,
 "allowedWrites":[],"timeoutSeconds":10}))' "$tooling_call_request_id_1" "$tooling_plugin_ref_1")"
tooling_call_1="$(curl -fsS -H 'content-type: application/json' -d "$tooling_call_payload_1" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/tool-calls")"
python3 -c 'import json,sys
r=json.loads(sys.argv[1]); assert not r["replayed"]
assert r["environmentFingerprint"]==sys.argv[2]
assert r["plugin"]["version"]=="1.0.0"
assert r["result"]["baseWorkspaceSnapshot"]=="sha256:"+"e"*64
assert r["result"]["resultWorkspaceSnapshot"]=="sha256:"+"e"*64' \
  "$tooling_call_1" "$tooling_fingerprint_1"
tooling_call_1_replay="$(curl -fsS -H 'content-type: application/json' -d "$tooling_call_payload_1" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/tool-calls")"
[[ "$(json_field "$tooling_call_1_replay" replayed)" == true ]]

tooling_wrong_plugin_payload="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"plugin":json.loads(sys.argv[2]),
 "toolName":"echo","input":{},"baseWorkspaceSnapshot":"sha256:"+"e"*64,
 "allowedWrites":[],"timeoutSeconds":10}))' "$(new_uuid)" "$tooling_plugin_ref_2")"
tooling_wrong_plugin_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' -d "$tooling_wrong_plugin_payload" \
  "$tooling_base/api/v1/projects/$tooling_project_1/sessions/$tooling_session_1/tool-calls")"
[[ "$tooling_wrong_plugin_status" == 409 ]]

tooling_call_payload_2="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"plugin":json.loads(sys.argv[2]),
 "toolName":"echo","input":{"library":"v2"},"baseWorkspaceSnapshot":"sha256:"+"f"*64,
 "allowedWrites":[],"timeoutSeconds":10}))' "$(new_uuid)" "$tooling_plugin_ref_2")"
tooling_call_2="$(curl -fsS -H 'content-type: application/json' -d "$tooling_call_payload_2" \
  "$tooling_base/api/v1/projects/$tooling_project_2/sessions/$tooling_session_2/tool-calls")"
[[ "$(json_field "$tooling_call_2" plugin.version)" == 2.0.0 ]]

tooling_child_response="$(post_goal_for "$tooling_project_1" session.propose_child \
  "{\"parentSessionId\":\"$tooling_session_1\",\"revision\":$tooling_goal_revision}")"
tooling_child_proposal="$(json_field "$tooling_child_response" result.proposalId)"
tooling_child_approval="$(post_goal_for "$tooling_project_1" proposal.approve \
  "{\"proposalId\":\"$tooling_child_proposal\",\"expectedRevision\":1,\"branchName\":\"继承环境子目标\",\"assignment\":\"验证继承\",\"agentIdentity\":\"tool-child\"}")"
tooling_child_session="$(json_field "$tooling_child_approval" result.sessionId)"
tooling_child_snapshot="$(curl -fsS \
  "$tooling_base/api/v1/projects/$tooling_project_1/goal-graph")"
python3 -c 'import json,sys
s=json.loads(sys.argv[1]); child=sys.argv[2]; fingerprint=sys.argv[3]
session=next(x for x in s["sessions"] if x["id"]==child)
assert session["environmentFingerprint"]==fingerprint' \
  "$tooling_child_snapshot" "$tooling_child_session" "$tooling_fingerprint_1"

tooling_plugin_draft_3="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p.pop("contentDigest"); p["version"]="3.0.0"; p["displayName"]="Fudian reference tools v3"; print(json.dumps(p))' \
  "$tooling_plugin_1")"
curl -fsS -H 'content-type: application/json' -d "$tooling_plugin_draft_3" \
  "$tooling_base/api/v1/plugins" >/dev/null
tooling_latest_3="$(curl -fsS -H 'content-type: application/json' \
  -d '{"pluginId":"fudian.tools.reference","version":"latest"}' \
  "$tooling_base/api/v1/plugins/resolve")"
[[ "$(json_field "$tooling_latest_3" version)" == 3.0.0 ]]
tooling_environment_2_after="$(curl -fsS \
  "$tooling_base/api/v1/environments/$tooling_environment_id_2")"
python3 -c 'import json,sys
e=json.loads(sys.argv[1]); assert e["fingerprint"]==sys.argv[2]
assert e["manifest"]["plugins"][0]["version"]=="2.0.0"' \
  "$tooling_environment_2_after" "$tooling_fingerprint_2"

docker exec -i "$tooling_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test <<'SQL' >/dev/null
DO $$
BEGIN
  IF (SELECT count(*) FROM plugin_packages) <> 3
     OR (SELECT count(*) FROM environment_manifests) <> 2
     OR (SELECT count(*) FROM tool_calls) <> 2
     OR (SELECT count(*) FROM session_environment_bindings) <> 3 THEN
    RAISE EXCEPTION 'unexpected tooling audit counts';
  END IF;
  BEGIN
    UPDATE plugin_packages SET version = '9.0.0';
    RAISE EXCEPTION 'immutable plugin package was updated';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;
SQL

echo "tooling HTTP flow passed: version coexistence, pinned latest, isolated environments, mock broker audit"
