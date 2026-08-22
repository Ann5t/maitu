#!/usr/bin/env bash
set -euo pipefail

real_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
real_suffix="$$"
real_network="fudian-real-plugins-test-$real_suffix"
real_db="fudian-real-plugins-db-$real_suffix"
real_app="fudian-real-plugins-app-$real_suffix"
real_tool_live="fudian-real-tool-live-$real_suffix"
real_tool_crash="fudian-real-tool-crash-$real_suffix"
real_tmp="$(mktemp -d)"
real_worker_bootstrap="real_plugin_worker_bootstrap_0123456789abcdef"

cleanup_real_plugins() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$real_app" >/dev/null 2>&1; then
    docker logs "$real_app" >&2 || true
  fi
  [[ "$real_tool_live" == fudian-real-tool-live-* ]] \
    && docker rm -f "$real_tool_live" >/dev/null 2>&1 || true
  [[ "$real_tool_crash" == fudian-real-tool-crash-* ]] \
    && docker rm -f "$real_tool_crash" >/dev/null 2>&1 || true
  [[ "$real_app" == fudian-real-plugins-app-* ]] \
    && docker rm -f "$real_app" >/dev/null 2>&1 || true
  [[ "$real_db" == fudian-real-plugins-db-* ]] \
    && docker rm -f "$real_db" >/dev/null 2>&1 || true
  [[ "$real_network" == fudian-real-plugins-test-* ]] \
    && docker network rm "$real_network" >/dev/null 2>&1 || true
  [[ "$real_tmp" == /tmp/tmp.* && -d "$real_tmp" ]] && rm -rf -- "$real_tmp"
  return "$exit_status"
}
trap cleanup_real_plugins EXIT

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

wait_for_real_app() {
  for real_attempt in $(seq 1 60); do
    if curl -fsS --connect-timeout 1 --max-time 2 \
      "$real_base/api/health" >/dev/null 2>&1; then
      return 0
    fi
    if [[ "$real_attempt" == 60 ]]; then
      docker logs "$real_app"
      return 1
    fi
    sleep 1
  done
}

start_real_app() {
  docker run -d --name "$real_app" --network "$real_network" \
    -p 127.0.0.1::3000 \
    -e DATABASE_URL=postgres://fudian_test:fudian_test_only@real-plugins-db:5432/fudian_test \
    -e FUDIAN_BIND=0.0.0.0:3000 \
    -e ARTIFACT_ROOT=/data/artifacts \
    -e REPOSITORY_ROOT=/data/repositories \
    -e WORKTREE_ROOT=/data/worktrees \
    -e RUNNER_OUTPUT_ROOT=/data/runner \
    -e RUNNER_RUNTIME_DIGEST="$real_runner_digest" \
    -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$real_worker_bootstrap" \
    -e RUST_LOG=fudian=info \
    --mount "type=bind,src=$real_repo_root,dst=/app" \
    --mount "type=bind,src=$real_tmp/artifacts,dst=/data/artifacts" \
    --mount "type=bind,src=$real_tmp/repositories,dst=/data/repositories" \
    --mount "type=bind,src=$real_tmp/worktrees,dst=/data/worktrees" \
    --mount "type=bind,src=$real_tmp/runner,dst=/data/runner" \
    --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
    --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
    --mount type=volume,src=fudian_rust_target,dst=/app/target \
    fudian-nextgen-app:latest cargo run --locked >/dev/null
  real_port="$(docker port "$real_app" 3000/tcp | sed -n 's/.*://p')"
  real_base="http://127.0.0.1:$real_port"
  wait_for_real_app
}

register_real_worker() {
  local worker_id="$1"
  local worker_token="$2"
  local capabilities="$3"
  local payload
  payload="$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"workerId":sys.argv[1],
 "workerToken":sys.argv[2],"displayName":"real plugin launcher",
 "capabilities":json.loads(sys.argv[3])}))' "$worker_id" "$worker_token" "$capabilities")"
  curl -fsS -H 'content-type: application/json' \
    -H "x-fudian-worker-bootstrap: $real_worker_bootstrap" \
    -d "$payload" "$real_base/api/v1/scheduler/workers"
}

claim_real_action() {
  local worker_id="$1"
  local worker_token="$2"
  local lease_token="$3"
  local soft_ttl="$4"
  curl -fsS -H 'content-type: application/json' \
    -d "{\"workerId\":\"$worker_id\",\"workerToken\":\"$worker_token\",\"clientRequestId\":\"$(new_uuid)\",\"leaseToken\":\"$lease_token\",\"softTtlSeconds\":$soft_ttl,\"hardTtlSeconds\":60}" \
    "$real_base/api/v1/scheduler/claim"
}

real_lease_credentials() {
  local claim="$1"
  local worker_id="$2"
  local worker_token="$3"
  local lease_token="$4"
  python3 -c 'import json,sys
c=json.loads(sys.argv[1]); print(json.dumps({"workerId":sys.argv[2],"workerToken":sys.argv[3],
 "leaseId":c["lease"]["id"],"leaseToken":sys.argv[4],"fencingToken":c["lease"]["fencingToken"]}))' \
    "$claim" "$worker_id" "$worker_token" "$lease_token"
}

launch_real_persistent_tool() {
  local container_name="$1"
  local claim="$2"
  local worktree_key="$3"
  local image_digest
  local entrypoint
  local source_path
  local cpu_millis
  local memory_mib
  local pids
  local cpus
  image_digest="$(json_field "$claim" action.payload.runtime.imageDigest)"
  entrypoint="$(json_field "$claim" action.payload.runtime.entrypoint)"
  source_path="$(json_field "$claim" action.payload.input.sourcePath)"
  cpu_millis="$(json_field "$claim" action.payload.resourcePolicy.cpuMillis)"
  memory_mib="$(json_field "$claim" action.payload.resourcePolicy.memoryMiB)"
  pids="$(json_field "$claim" action.payload.resourcePolicy.pids)"
  cpus="$(python3 -c 'import sys; print(int(sys.argv[1])/1000)' "$cpu_millis")"
  [[ "$image_digest" == "$(image_id "$real_playwright_image")" ]]
  [[ "$entrypoint" == /runtime/fudian-tool-runtime ]]
  docker run -d --name "$container_name" --network "$real_network" \
    --network-alias "$container_name" \
    -p 127.0.0.1::4173 \
    --read-only \
    --user 1000:1000 \
    --cap-drop ALL \
    --security-opt no-new-privileges:true \
    --pids-limit "$pids" \
    --memory "${memory_mib}m" \
    --cpus "$cpus" \
    --tmpfs /tmp:rw,nosuid,nodev,size=33554432,mode=1777 \
    -e FUDIAN_INPUT=/workspace/input \
    --mount "type=bind,src=$real_tmp/worktrees/$worktree_key,dst=/workspace/input,readonly" \
    --entrypoint "$entrypoint" \
    "$image_digest" serve-static "$source_path" 4173 >/dev/null
}

wait_for_real_tool() {
  local container_name="$1"
  local port
  port="$(docker port "$container_name" 4173/tcp | sed -n 's/.*://p')"
  for real_tool_attempt in $(seq 1 30); do
    if curl -fsS "http://127.0.0.1:$port/health" >/dev/null 2>&1; then
      printf '%s' "$port"
      return 0
    fi
    if [[ "$real_tool_attempt" == 30 ]]; then
      docker logs "$container_name"
      return 1
    fi
    sleep 1
  done
}

post_goal_for() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  local body
  body="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"action":sys.argv[2],"payload":json.loads(sys.argv[3])},ensure_ascii=False))' \
    "$(new_uuid)" "$action" "$payload")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$real_base/api/v1/projects/$project_id/goal-commands"
}

create_project_session() {
  local intent="$1"
  local agent="$2"
  local project
  local project_id
  local proposal
  local proposal_id
  local approval
  project="$(curl -fsS -H 'content-type: application/json' \
    -d "{\"intent\":\"$intent\"}" "$real_base/api/projects")"
  project_id="$(json_field "$project" id)"
  proposal="$(post_goal_for "$project_id" proposal.create "{\"revision\":$real_revision}")"
  proposal_id="$(json_field "$proposal" result.proposalId)"
  post_goal_for "$project_id" proposal.submit \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1}" >/dev/null
  approval="$(post_goal_for "$project_id" proposal.approve \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1,\"branchName\":\"真实插件目标\",\"assignment\":\"验证中央工具运行\",\"agentIdentity\":\"$agent\"}")"
  printf '%s|%s|%s|%s|%s' \
    "$project_id" \
    "$(json_field "$approval" result.goalBranchId)" \
    "$(json_field "$approval" result.sessionId)" \
    "$(json_field "$approval" result.workspace.workspace.worktreeKey)" \
    "$(json_field "$approval" result.workspace.workspace.workspaceSnapshot)"
}

execute_seed_worker() {
  local prepare="$1"
  local worktree_key="$2"
  local label="$3"
  local spec_file="$real_tmp/$label-spec.json"
  local output_key
  local output_path
  output_key="$(json_field "$prepare" outputKey)"
  output_path="$real_tmp/runner/$output_key"
  python3 -c 'import json,sys
with open(sys.argv[2],"w",encoding="utf-8") as handle:
    json.dump(json.loads(sys.argv[1])["spec"],handle,separators=(",",":"),ensure_ascii=False)' \
    "$prepare" "$spec_file"
  chmod 0777 "$output_path"
  docker run --rm \
    --network none \
    --read-only \
    --cap-drop ALL \
    --security-opt no-new-privileges:true \
    --pids-limit 32 \
    --memory 128m \
    --cpus 0.5 \
    --tmpfs /tmp:rw,nosuid,nodev,size=33554432 \
    --mount "type=bind,src=$real_tmp/worktrees/$worktree_key,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$output_path,dst=/workspace/output" \
    --mount "type=bind,src=$spec_file,dst=/workspace/result/spec.json,readonly" \
    fudian-nextgen-runner:latest execute /workspace/result/spec.json
}

seed_file() {
  local project_id="$1"
  local session_id="$2"
  local worktree_key="$3"
  local snapshot="$4"
  local path="$5"
  local content="$6"
  local label="$7"
  local request_id
  local payload
  local prepare
  local result
  local token
  local job_id
  local finalize_payload
  local finalize
  request_id="$(new_uuid)"
  payload="$(python3 -c 'import json,sys
print(json.dumps({
 "clientRequestId":sys.argv[1],"baseWorkspaceSnapshot":sys.argv[2],
 "allowedWrites":[sys.argv[3].split("/")[0]+"/**"],
 "capabilities":{"network":"denied","externalWrites":[],"accountReferences":[],"paidOperations":False,"deployment":False},
 "resources":{"cpuMillis":500,"memoryMiB":128,"diskMiB":16,"pids":32,"timeoutSeconds":10,"stdoutBytes":4096,"stderrBytes":4096},
 "command":{"program":"/usr/local/bin/fudian-runner","args":["fixture-write",sys.argv[3],sys.argv[4]],"environment":{}}
},ensure_ascii=False))' "$request_id" "$snapshot" "$path" "$content")"
  prepare="$(curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$real_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs")"
  result="$(execute_seed_worker "$prepare" "$worktree_key" "$label")"
  token="$(json_field "$prepare" leaseToken)"
  job_id="$(json_field "$prepare" jobId)"
  finalize_payload="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])}))' \
    "$token" "$result")"
  finalize="$(curl -fsS -H 'content-type: application/json' -d "$finalize_payload" \
    "$real_base/api/v1/projects/$project_id/sessions/$session_id/runner-jobs/$job_id/finalize")"
  [[ "$(json_field "$finalize" status)" == succeeded ]]
  json_field "$finalize" workspaceSnapshot
}

image_id() {
  docker image inspect --format '{{.Id}}' "$1"
}

entry_digest() {
  local bare
  bare="$(docker run --rm --entrypoint sha256sum "$1" /runtime/fudian-tool-runtime | awk '{print $1}')"
  printf 'sha256:%s' "$bare"
}

plugin_draft() {
  python3 - "$@" <<'PY'
import json
import sys

plugin_id, version, display_name, runtime_digest, tool_name = sys.argv[1:]
common_string = {"type": "string", "maxLength": 1000}
properties = {"sourcePath": common_string, "reportPath": common_string}
required = ["sourcePath", "reportPath"]
resource = {
    "cpuMillis": 500,
    "memoryMiB": 256,
    "diskMiB": 64,
    "pids": 64,
    "timeoutSeconds": 60,
    "stdoutBytes": 65536,
    "stderrBytes": 65536,
}
if plugin_id == "fudian.tools.cxx":
    properties["language"] = {"type": "string", "enum": ["c", "cpp"]}
    required.append("language")
elif plugin_id == "fudian.tools.python":
    properties["arguments"] = {
        "type": "array",
        "maxItems": 50,
        "items": {"type": "string", "maxLength": 1000},
    }
elif plugin_id == "fudian.tools.playwright":
    properties["screenshotPath"] = common_string
    required.append("screenshotPath")
    resource.update({"cpuMillis": 1000, "memoryMiB": 1024, "pids": 128})
elif plugin_id == "fudian.tools.pptmaster":
    properties = {"sourcePath": common_string, "outputPath": common_string}
    required = ["sourcePath", "outputPath"]

tools = [{
    "name": tool_name,
    "description": f"Execute {display_name}",
    "inputSchema": {
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": False,
    },
    "outputSchema": {"type": "object"},
    "idempotency": "safe",
}]
if plugin_id == "fudian.tools.playwright":
    tools.append({
        "name": "serve",
        "description": "Serve an immutable local HTML preview for a bounded ToolLease",
        "inputSchema": {
            "type": "object",
            "properties": {"sourcePath": common_string},
            "required": ["sourcePath"],
            "additionalProperties": False,
        },
        "outputSchema": {"type": "object"},
        "idempotency": "safe",
        "persistent": {
            "adapterKind": "http_static_preview",
            "endpointKinds": ["http_preview"],
            "startupRetrySafety": "safe",
            "maxDurationSeconds": 600,
        },
    })

print(json.dumps({
    "schemaVersion": 1,
    "pluginId": plugin_id,
    "version": version,
    "displayName": display_name,
    "description": f"Pinned central runtime for {display_name}",
    "capabilities": [f"{plugin_id}.{tool_name}"],
    "permissions": {
        "network": "denied",
        "workspaceRead": ["**"],
        "workspaceWrite": ["reports/**"],
        "externalWrites": False,
    },
    "tools": tools,
    "skill": {"entry": "SKILL.md"},
    "runtime": {
        "kind": "oci",
        "contentDigest": runtime_digest,
        "entrypoint": "/runtime/fudian-tool-runtime",
    },
    "assets": [],
    "resourceHints": resource,
}, separators=(",", ":")))
PY
}

seal_draft() {
  curl -fsS -H 'content-type: application/json' -d "$1" "$real_base/api/v1/plugins/seal"
}

install_manifest() {
  local manifest="$1"
  local entry="$2"
  local label="$3"
  local self_test
  local preview_payload
  local preview
  local statement_digest
  local signature
  local install_payload
  local installation
  local replay
  self_test="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"status":"passed","runnerDigest":sys.argv[1],
 "runtimeEntryDigest":sys.argv[2],"checks":{"directSmoke":True,"label":sys.argv[3]}}))' \
    "$real_runner_digest" "$entry" "$label")"
  preview_payload="$(python3 -c 'import json,sys
print(json.dumps({"manifest":json.loads(sys.argv[1]),"publisherId":"fudian.local","selfTest":json.loads(sys.argv[2])}))' \
    "$manifest" "$self_test")"
  preview="$(curl -fsS -H 'content-type: application/json' -d "$preview_payload" \
    "$real_base/api/v1/plugins/install-statement")"
  statement_digest="$(json_field "$preview" statementDigest)"
  printf '%s' "$statement_digest" > "$real_tmp/$label-statement.txt"
  openssl pkeyutl -sign -rawin -inkey "$real_tmp/publisher-private.pem" \
    -in "$real_tmp/$label-statement.txt" -out "$real_tmp/$label-signature.bin"
  signature="$(od -An -v -tx1 "$real_tmp/$label-signature.bin" | tr -d ' \n')"
  install_payload="$(python3 -c 'import json,sys
print(json.dumps({"manifest":json.loads(sys.argv[1]),"publisherId":"fudian.local",
 "signature":sys.argv[2],"selfTest":json.loads(sys.argv[3])}))' \
    "$manifest" "$signature" "$self_test")"
  installation="$(curl -fsS -H 'content-type: application/json' -d "$install_payload" \
    "$real_base/api/v1/plugins/install")"
  replay="$(curl -fsS -H 'content-type: application/json' -d "$install_payload" \
    "$real_base/api/v1/plugins/install")"
  [[ "$(json_field "$installation" id)" == "$(json_field "$replay" id)" ]]
  [[ "$(json_field "$replay" statementDigest)" == "$(json_field "$installation" statementDigest)" ]]
  printf '%s' "$installation"
}

plugin_ref() {
  python3 -c 'import json,sys
m=json.loads(sys.argv[1]); print(json.dumps({"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},separators=(",",":")))' "$1"
}

create_environment() {
  local plugins_json="$1"
  local toolchains_json="$2"
  local payload
  payload="$(python3 -c 'import json,sys
print(json.dumps({
 "schemaVersion":1,
 "baseRuntime":{"kind":"fudian-runner","digest":sys.argv[1]},
 "plugins":json.loads(sys.argv[2]),"toolchains":json.loads(sys.argv[3]),
 "dependencyLocks":[],"targetPlatform":"linux-amd64","features":[],"buildParameters":{},
 "networkPolicy":"denied",
 "resourcePolicy":{"cpuMillis":2000,"memoryMiB":2048,"diskMiB":512,"pids":192,"timeoutSeconds":300,"stdoutBytes":262144,"stderrBytes":262144},
 "environmentPolicy":{"allowedNames":[],"secretReferences":[]}
},separators=(",",":")))' "$real_runner_digest" "$plugins_json" "$toolchains_json")"
  curl -fsS -H 'content-type: application/json' -d "$payload" "$real_base/api/v1/environments"
}

bind_environment() {
  local project_id="$1"
  local session_id="$2"
  local environment_id="$3"
  curl -fsS -H 'content-type: application/json' \
    -d "{\"clientRequestId\":\"$(new_uuid)\",\"environmentManifestId\":\"$environment_id\"}" \
    "$real_base/api/v1/projects/$project_id/sessions/$session_id/environment"
}

run_real_tool() {
  local project_id="$1"
  local session_id="$2"
  local worktree_key="$3"
  local snapshot="$4"
  local image="$5"
  local manifest="$6"
  local tool_name="$7"
  local input_json="$8"
  local label="$9"
  local request_id
  local payload
  local prepare
  local execution_id
  local job_id
  local lease_token
  local output_key
  local output_path
  local spec_file
  local expected_image
  local cpu_millis
  local memory_mib
  local pids
  local cpus
  local result
  local finalize_payload
  local finalize_url
  local finalize
  request_id="$(new_uuid)"
  payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({
 "clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":sys.argv[3],"input":json.loads(sys.argv[4]),
 "baseWorkspaceSnapshot":sys.argv[5],"allowedWrites":["reports/**"],
 "deletePaths":[],"timeoutSeconds":60
},separators=(",",":")))' "$request_id" "$manifest" "$tool_name" "$input_json" "$snapshot")"
  prepare="$(curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$real_base/api/v1/projects/$project_id/sessions/$session_id/tool-executions")"
  expected_image="$(image_id "$image")"
  [[ "$(json_field "$prepare" runtimeImageDigest)" == "$expected_image" ]]
  [[ "$(json_field "$prepare" runner.spec.runtimeEntryDigest)" == "$(entry_digest "$image")" ]]
  if [[ "$label" == rust ]]; then
    local replay
    replay="$(curl -fsS -H 'content-type: application/json' -d "$payload" \
      "$real_base/api/v1/projects/$project_id/sessions/$session_id/tool-executions")"
    [[ "$(json_field "$replay" replayed)" == true ]]
    [[ "$(json_field "$replay" runner.leaseToken)" == null ]]
    [[ "$(json_field "$replay" executionId)" == "$(json_field "$prepare" executionId)" ]]
  fi
  execution_id="$(json_field "$prepare" executionId)"
  job_id="$(json_field "$prepare" runner.jobId)"
  lease_token="$(json_field "$prepare" runner.leaseToken)"
  output_key="$(json_field "$prepare" runner.outputKey)"
  output_path="$real_tmp/runner/$output_key"
  spec_file="$real_tmp/$label-real-spec.json"
  python3 -c 'import json,sys
with open(sys.argv[2],"w",encoding="utf-8") as handle:
    json.dump(json.loads(sys.argv[1])["runner"]["spec"],handle,separators=(",",":"),ensure_ascii=False)' \
    "$prepare" "$spec_file"
  chmod 0777 "$output_path"
  cpu_millis="$(json_field "$prepare" runner.spec.resources.cpuMillis)"
  memory_mib="$(json_field "$prepare" runner.spec.resources.memoryMiB)"
  pids="$(json_field "$prepare" runner.spec.resources.pids)"
  cpus="$(python3 -c 'import sys; print(int(sys.argv[1])/1000)' "$cpu_millis")"
  [[ "$(image_id "$image")" == "$(json_field "$prepare" runtimeImageDigest)" ]]
  result="$(docker run --rm \
    --network none \
    --read-only \
    --cap-drop ALL \
    --security-opt no-new-privileges:true \
    --pids-limit "$pids" \
    --memory "${memory_mib}m" \
    --cpus "$cpus" \
    --tmpfs /tmp:rw,nosuid,nodev,size=268435456,mode=1777 \
    --mount "type=bind,src=$real_tmp/worktrees/$worktree_key,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$output_path,dst=/workspace/output" \
    --mount "type=bind,src=$spec_file,dst=/workspace/result/spec.json,readonly" \
    "$image" execute /workspace/result/spec.json)"
  python3 -c 'import json,sys
r=json.loads(sys.argv[1]); assert r["status"]=="succeeded", r
i=r["isolation"]
assert i["runtimeEntryDigest"]==sys.argv[2]
assert i["networkIsolated"] and i["noNewPrivileges"] and i["rootReadOnly"]
assert i["inputReadOnly"] and i["outputWritable"]
assert i["dockerSocketAbsent"] and i["hostHomeAbsent"]
assert set(i["effectiveCapabilitiesHex"]) <= {"0"}' \
    "$result" "$(entry_digest "$image")"
  finalize_payload="$(python3 -c 'import json,sys
print(json.dumps({"leaseToken":sys.argv[1],"result":json.loads(sys.argv[2])},separators=(",",":")))' \
    "$lease_token" "$result")"
  finalize_url="$real_base/api/v1/projects/$project_id/sessions/$session_id/tool-executions/$execution_id/runner-jobs/$job_id/finalize"
  if [[ "$label" == rust ]]; then
    local forged_payload
    local forged_status
    forged_payload="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["result"]["isolation"]["runtimeEntryDigest"]="sha256:"+"0"*64
print(json.dumps(p,separators=(",",":")))' "$finalize_payload")"
    forged_status="$(curl -sS -o "$real_tmp/forged-entry.json" -w '%{http_code}' \
      -H 'content-type: application/json' -d "$forged_payload" "$finalize_url")"
    [[ "$forged_status" == 403 ]]
    [[ "$(json_field "$(<"$real_tmp/forged-entry.json")" code)" == unsafe_runner_attestation ]]
  fi
  finalize="$(curl -fsS -H 'content-type: application/json' -d "$finalize_payload" "$finalize_url")"
  [[ "$(json_field "$finalize" runner.status)" == succeeded ]]
  [[ "$(json_field "$finalize" toolCall.result.status)" == succeeded ]]
  [[ "$(json_field "$finalize" toolCall.result.environmentFingerprint)" \
      == "$(json_field "$finalize" toolCall.environmentFingerprint)" ]]
  if [[ "$label" == rust ]]; then
    local head_before_replay
    local finalize_replay
    head_before_replay="$(json_field "$finalize" runner.headCommit)"
    finalize_replay="$(curl -fsS -H 'content-type: application/json' -d "$finalize_payload" "$finalize_url")"
    [[ "$(json_field "$finalize_replay" replayed)" == true ]]
    [[ "$(json_field "$finalize_replay" runner.headCommit)" == "$head_before_replay" ]]
  fi
  printf '%s' "$finalize"
}

if [[ "${SKIP_PLUGIN_IMAGE_SMOKE:-0}" != 1 ]]; then
  ./scripts/test-plugin-images.sh
fi

mkdir -p "$real_tmp/artifacts" "$real_tmp/repositories" "$real_tmp/worktrees" "$real_tmp/runner"
chmod 0777 "$real_tmp/artifacts" "$real_tmp/repositories" "$real_tmp/worktrees" "$real_tmp/runner"

docker network create "$real_network" >/dev/null
docker run -d --name "$real_db" --network "$real_network" \
  --network-alias real-plugins-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for real_attempt in $(seq 1 30); do
  if docker exec "$real_db" pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test \
    >/dev/null 2>&1; then
    break
  fi
  [[ "$real_attempt" == 30 ]] && docker logs "$real_db" && exit 1
  sleep 1
done

real_runner_digest="$(docker run --rm --entrypoint /usr/local/bin/fudian-runner \
  fudian-nextgen-runner:latest digest)"

start_real_app

openssl genpkey -algorithm Ed25519 -out "$real_tmp/publisher-private.pem" >/dev/null 2>&1
openssl pkey -in "$real_tmp/publisher-private.pem" -pubout -outform DER \
  -out "$real_tmp/publisher-public.der" >/dev/null 2>&1
real_public_key="$(tail -c 32 "$real_tmp/publisher-public.der" | od -An -v -tx1 | tr -d ' \n')"
real_publisher="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"publisherId\":\"fudian.local\",\"displayName\":\"Fudian local test publisher\",\"publicKey\":\"$real_public_key\"}" \
  "$real_base/api/v1/plugin-publishers")"
[[ "$(json_field "$real_publisher" status)" == active ]]

real_rust_image=fudian-plugin-rust:1.0.0
real_cxx_image=fudian-plugin-cxx:1.0.0
real_python_1_image=fudian-plugin-python:1.0.0
real_python_2_image=fudian-plugin-python:2.0.0
real_playwright_image=fudian-plugin-playwright:1.0.0
for image in "$real_rust_image" "$real_cxx_image" "$real_python_1_image" \
  "$real_python_2_image" "$real_playwright_image"; do
  [[ "$(docker run --rm --entrypoint /usr/local/bin/fudian-runner "$image" digest)" \
      == "$real_runner_digest" ]]
done

real_rust_manifest="$(seal_draft "$(plugin_draft fudian.tools.rust 1.0.0 'Rust checks' "$(image_id "$real_rust_image")" check)")"
real_cxx_manifest="$(seal_draft "$(plugin_draft fudian.tools.cxx 1.0.0 'C and C++ checks' "$(image_id "$real_cxx_image")" check)")"
real_python_1_manifest="$(seal_draft "$(plugin_draft fudian.tools.python 1.0.0 'Python legacy runtime' "$(image_id "$real_python_1_image")" run)")"
real_python_2_manifest="$(seal_draft "$(plugin_draft fudian.tools.python 2.0.0 'Python modern runtime' "$(image_id "$real_python_2_image")" run)")"
real_playwright_manifest="$(seal_draft "$(plugin_draft fudian.tools.playwright 1.0.0 'Playwright Chromium' "$(image_id "$real_playwright_image")" inspect)")"

real_rust_self_test="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"status":"passed","runnerDigest":sys.argv[1],"runtimeEntryDigest":sys.argv[2],"checks":{"directSmoke":True}}))' \
  "$real_runner_digest" "$(entry_digest "$real_rust_image")")"
real_bad_runner_self_test="$(python3 -c 'import json,sys
print(json.dumps({"schemaVersion":1,"status":"passed","runnerDigest":"sha256:"+"0"*64,"runtimeEntryDigest":sys.argv[1],"checks":{}}))' \
  "$(entry_digest "$real_rust_image")")"
real_bad_runner_payload="$(python3 -c 'import json,sys
print(json.dumps({"manifest":json.loads(sys.argv[1]),"publisherId":"fudian.local","signature":"0"*128,"selfTest":json.loads(sys.argv[2])}))' \
  "$real_rust_manifest" "$real_bad_runner_self_test")"
real_bad_runner_status="$(curl -sS -o "$real_tmp/bad-runner.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_bad_runner_payload" \
  "$real_base/api/v1/plugins/install")"
[[ "$real_bad_runner_status" == 409 ]]
[[ "$(json_field "$(<"$real_tmp/bad-runner.json")" code)" == runner_digest_mismatch ]]

real_bad_signature_payload="$(python3 -c 'import json,sys
print(json.dumps({"manifest":json.loads(sys.argv[1]),"publisherId":"fudian.local","signature":"0"*128,"selfTest":json.loads(sys.argv[2])}))' \
  "$real_rust_manifest" "$real_rust_self_test")"
real_bad_signature_status="$(curl -sS -o "$real_tmp/bad-signature.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_bad_signature_payload" \
  "$real_base/api/v1/plugins/install")"
[[ "$real_bad_signature_status" == 403 ]]
[[ "$(json_field "$(<"$real_tmp/bad-signature.json")" code)" == invalid_plugin_signature ]]

real_rust_install="$(install_manifest "$real_rust_manifest" "$(entry_digest "$real_rust_image")" rust)"
real_cxx_install="$(install_manifest "$real_cxx_manifest" "$(entry_digest "$real_cxx_image")" cxx)"
real_python_1_install="$(install_manifest "$real_python_1_manifest" "$(entry_digest "$real_python_1_image")" python1)"
real_python_2_install="$(install_manifest "$real_python_2_manifest" "$(entry_digest "$real_python_2_image")" python2)"
real_playwright_install="$(install_manifest "$real_playwright_manifest" "$(entry_digest "$real_playwright_image")" playwright)"
[[ "$(json_field "$real_rust_install" runtimeImageDigest)" == "$(image_id "$real_rust_image")" ]]

real_rust_proof="$(curl -fsS "$real_base/api/v1/plugins/fudian.tools.rust/1.0.0/install-proof")"
[[ "$(json_field "$real_rust_proof" installation.id)" == "$(json_field "$real_rust_install" id)" ]]
[[ "$(json_field "$real_rust_proof" publisher.status)" == active ]]
real_catalog="$(curl -fsS "$real_base/api/v1/plugins")"
python3 -c 'import json,sys
entries=json.loads(sys.argv[1])["plugins"]
rust=next(item for item in entries if item["plugin"]["pluginId"]=="fudian.tools.rust")
assert rust["installed"] and rust["publisherId"]=="fudian.local"
assert "tools" not in rust and "runtime" not in rust and "permissions" not in rust' "$real_catalog"

real_revision='{
  "whyNeeded":"验证中央签名插件与独立工具环境",
  "contract":{
    "desiredOutcome":"真实工具只在固定 OCI 环境中执行并经 Git CAS 回写",
    "hardConstraints":["断网","单写 Lease","不污染 worktree"],
    "subjectivePreferences":[],"unknowns":[],"nonGoals":["不下载工具","不访问生产"],
    "validationPlan":["运行 Rust、Python、C++ 与 Chromium"],"judgmentTriggers":[],
    "stopConditions":["签名、隔离、版本冲突和撤销均通过"],
    "expectedContributions":["Runner 与 ToolCall 绑定证据"]
  },
  "expectedContributions":["真实工具报告"],"explorationPlan":[],"contextInheritance":{},
  "toolRequirements":["fudian.tools.rust","fudian.tools.python","fudian.tools.cxx","fudian.tools.playwright"],
  "capabilityPolicy":{
    "network":"denied","networkDestinations":[],"externalWrites":[],"accountReferences":[],
    "paidOperations":false,"deployment":false,
    "readScopes":["current_worktree","parent_snapshot"],"writePaths":["**"],
    "maximumResources":{"cpuMillis":2000,"memoryMiB":2048,"diskMiB":512,"pids":192,"timeoutSeconds":300,"stdoutBytes":262144,"stderrBytes":262144}
  },
  "inferences":[],"revisionReason":null
}'

IFS='|' read -r real_project_1 real_branch_1 real_session_1 real_worktree_1 real_snapshot_1 \
  <<< "$(create_project_session '验证 Rust C++ Python1 Playwright 工具环境' plugin-agent-1)"
real_snapshot_1="$(seed_file "$real_project_1" "$real_session_1" "$real_worktree_1" "$real_snapshot_1" \
  src/valid.rs "$(<"$real_repo_root/tests/fixtures/plugins/valid.rs")" seed-rust)"
real_snapshot_1="$(seed_file "$real_project_1" "$real_session_1" "$real_worktree_1" "$real_snapshot_1" \
  src/valid.cpp "$(<"$real_repo_root/tests/fixtures/plugins/valid.cpp")" seed-cxx)"
real_snapshot_1="$(seed_file "$real_project_1" "$real_session_1" "$real_worktree_1" "$real_snapshot_1" \
  src/library_probe.py "$(<"$real_repo_root/tests/fixtures/plugins/library_probe.py")" seed-python1)"
real_snapshot_1="$(seed_file "$real_project_1" "$real_session_1" "$real_worktree_1" "$real_snapshot_1" \
  web/page.html "$(<"$real_repo_root/tests/fixtures/plugins/page.html")" seed-page)"

real_plugins_1="$(python3 -c 'import json,sys
print(json.dumps([json.loads(value) for value in sys.argv[1:]]))' \
  "$(plugin_ref "$real_rust_manifest")" "$(plugin_ref "$real_cxx_manifest")" \
  "$(plugin_ref "$real_python_1_manifest")" "$(plugin_ref "$real_playwright_manifest")")"
real_environment_1="$(create_environment "$real_plugins_1" \
  '{"rust":"1.97.1","gcc":"15","python":"3.13.15","playwright":"1.62.0"}')"
bind_environment "$real_project_1" "$real_session_1" "$(json_field "$real_environment_1" id)" >/dev/null

real_schema_payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"check","input":{"sourcePath":"src/valid.rs"},
 "baseWorkspaceSnapshot":sys.argv[3],"allowedWrites":["reports/**"],"deletePaths":[],"timeoutSeconds":60}))' \
  "$(new_uuid)" "$real_rust_manifest" "$real_snapshot_1")"
real_schema_status="$(curl -sS -o "$real_tmp/schema-error.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_schema_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-executions")"
[[ "$real_schema_status" == 422 ]]
[[ "$(json_field "$(<"$real_tmp/schema-error.json")" code)" == tool_input_schema_mismatch ]]

real_permission_payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"check","input":{"sourcePath":"src/valid.rs","reportPath":"reports/rust.json"},
 "baseWorkspaceSnapshot":sys.argv[3],"allowedWrites":["outside/**"],"deletePaths":[],"timeoutSeconds":60}))' \
  "$(new_uuid)" "$real_rust_manifest" "$real_snapshot_1")"
real_permission_status="$(curl -sS -o "$real_tmp/permission-error.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_permission_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-executions")"
[[ "$real_permission_status" == 409 ]]
[[ "$(json_field "$(<"$real_tmp/permission-error.json")" code)" == tool_not_allowed ]]

real_rust_finalize="$(run_real_tool "$real_project_1" "$real_session_1" "$real_worktree_1" \
  "$real_snapshot_1" "$real_rust_image" "$real_rust_manifest" check \
  '{"sourcePath":"src/valid.rs","reportPath":"reports/rust.json"}' rust)"
real_snapshot_1="$(json_field "$real_rust_finalize" runner.workspaceSnapshot)"
real_cxx_finalize="$(run_real_tool "$real_project_1" "$real_session_1" "$real_worktree_1" \
  "$real_snapshot_1" "$real_cxx_image" "$real_cxx_manifest" check \
  '{"sourcePath":"src/valid.cpp","language":"cpp","reportPath":"reports/cxx.json"}' cxx)"
real_snapshot_1="$(json_field "$real_cxx_finalize" runner.workspaceSnapshot)"
real_python_1_finalize="$(run_real_tool "$real_project_1" "$real_session_1" "$real_worktree_1" \
  "$real_snapshot_1" "$real_python_1_image" "$real_python_1_manifest" run \
  '{"sourcePath":"src/library_probe.py","reportPath":"reports/python.json"}' python1)"
real_snapshot_1="$(json_field "$real_python_1_finalize" runner.workspaceSnapshot)"
real_playwright_finalize="$(run_real_tool "$real_project_1" "$real_session_1" "$real_worktree_1" \
  "$real_snapshot_1" "$real_playwright_image" "$real_playwright_manifest" inspect \
  '{"sourcePath":"web/page.html","screenshotPath":"reports/page.png","reportPath":"reports/playwright.json"}' playwright)"
real_snapshot_1="$(json_field "$real_playwright_finalize" runner.workspaceSnapshot)"

IFS='|' read -r real_project_2 real_branch_2 real_session_2 real_worktree_2 real_snapshot_2 \
  <<< "$(create_project_session '验证 Python2 冲突依赖隔离' plugin-agent-2)"
real_snapshot_2="$(seed_file "$real_project_2" "$real_session_2" "$real_worktree_2" "$real_snapshot_2" \
  src/library_probe.py "$(<"$real_repo_root/tests/fixtures/plugins/library_probe.py")" seed-python2)"
real_plugins_2="[$(plugin_ref "$real_python_2_manifest")]"
real_environment_2="$(create_environment "$real_plugins_2" '{"python":"3.13.15-modern"}')"
bind_environment "$real_project_2" "$real_session_2" "$(json_field "$real_environment_2" id)" >/dev/null
real_python_2_finalize="$(run_real_tool "$real_project_2" "$real_session_2" "$real_worktree_2" \
  "$real_snapshot_2" "$real_python_2_image" "$real_python_2_manifest" run \
  '{"sourcePath":"src/library_probe.py","reportPath":"reports/python.json"}' python2)"
real_snapshot_2="$(json_field "$real_python_2_finalize" runner.workspaceSnapshot)"

python3 - "$real_tmp/worktrees/$real_worktree_1" "$real_tmp/worktrees/$real_worktree_2" <<'PY'
import json
import pathlib
import sys

one, two = map(pathlib.Path, sys.argv[1:])
for path in [one / "reports/rust.json", one / "reports/cxx.json", one / "reports/playwright.json", two / "reports/python.json"]:
    assert json.loads(path.read_text())["succeeded"], path
legacy = json.loads((one / "reports/python.json").read_text())["stdout"]
modern = json.loads((two / "reports/python.json").read_text())["stdout"]
assert "legacy:sample" in legacy and "modern:sample" in modern
assert (one / "reports/page.png").stat().st_size > 1000
for root in (one, two):
    forbidden = {"target", ".venv", "venv", "node_modules", "__pycache__", ".cache", "ms-playwright"}
    assert not any(path.name in forbidden for path in root.rglob("*")), root
PY
[[ -z "$(docker exec "$real_app" git -C "/data/worktrees/$real_worktree_1" status --porcelain)" ]]
[[ -z "$(docker exec "$real_app" git -C "/data/worktrees/$real_worktree_2" status --porcelain)" ]]
[[ "$(json_field "$real_environment_1" fingerprint)" != "$(json_field "$real_environment_2" fingerprint)" ]]

real_tool_worker="$(new_uuid)"
real_tool_worker_token="real_tool_worker_token_0123456789abcdef"
real_reconcile_worker="$(new_uuid)"
real_reconcile_worker_token="real_reconcile_worker_token_0123456789abcdef"
register_real_worker "$real_tool_worker" "$real_tool_worker_token" '["tool.lease"]' >/dev/null
register_real_worker "$real_reconcile_worker" "$real_reconcile_worker_token" \
  '["scheduler.reconcile","tool.cleanup"]' >/dev/null

real_live_request_id="$(new_uuid)"
real_live_payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"serve","input":{"sourcePath":"web/page.html"},
 "baseWorkspaceSnapshot":sys.argv[3],"softTtlSeconds":10,"durationSeconds":60}))' \
  "$real_live_request_id" "$real_playwright_manifest" "$real_snapshot_1")"
real_live_lease="$(curl -fsS -H 'content-type: application/json' -d "$real_live_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-leases")"
real_live_lease_id="$(json_field "$real_live_lease" lease.id)"
real_live_action_id="$(json_field "$real_live_lease" action.id)"
real_live_renewal_token="$(json_field "$real_live_lease" renewalToken)"
[[ "$(json_field "$real_live_lease" lease.status)" == requested ]]
[[ "$(json_field "$real_live_lease" action.payload.runtime.imageDigest)" \
    == "$(image_id "$real_playwright_image")" ]]
real_live_replay="$(curl -fsS -H 'content-type: application/json' -d "$real_live_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-leases")"
[[ "$(json_field "$real_live_replay" replayed)" == true ]]
[[ "$(json_field "$real_live_replay" renewalToken)" == null ]]
[[ "$(json_field "$real_live_replay" lease.id)" == "$real_live_lease_id" ]]

real_live_action_token="real_live_action_token_0123456789abcdef"
real_live_claim="$(claim_real_action "$real_tool_worker" "$real_tool_worker_token" \
  "$real_live_action_token" 30)"
[[ "$(json_field "$real_live_claim" action.id)" == "$real_live_action_id" ]]
real_live_credentials="$(real_lease_credentials "$real_live_claim" "$real_tool_worker" \
  "$real_tool_worker_token" "$real_live_action_token")"
launch_real_persistent_tool "$real_tool_live" "$real_live_claim" "$real_worktree_1"
real_tool_live_port="$(wait_for_real_tool "$real_tool_live")"
real_live_endpoint="http://$real_tool_live:4173"
real_live_activate="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["endpointRefs"]=[sys.argv[2]]; print(json.dumps(p))' \
  "$real_live_credentials" "$real_live_endpoint")"
real_live_active="$(curl -fsS -H 'content-type: application/json' -d "$real_live_activate" \
  "$real_base/api/v1/scheduler/action-runs/$real_live_action_id/tool-lease/activate")"
[[ "$(json_field "$real_live_active" status)" == active ]]
[[ "$(curl -fsS "http://127.0.0.1:$real_tool_live_port/")" == *"真实 Playwright Worker"* ]]
[[ "$(docker inspect --format '{{.HostConfig.ReadonlyRootfs}}' "$real_tool_live")" == true ]]
docker exec "$real_tool_live" sh -c 'test ! -e /var/run/docker.sock'
if docker exec "$real_tool_live" sh -c 'printf unsafe > /workspace/input/forbidden.txt' \
  >/dev/null 2>&1; then
  echo "persistent tool unexpectedly modified its read-only input" >&2
  exit 1
fi

docker kill "$real_app" >/dev/null
docker rm "$real_app" >/dev/null
start_real_app
[[ "$(curl -fsS "http://127.0.0.1:$real_tool_live_port/health")" == ok ]]
real_live_heartbeat="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["extendSeconds"]=30; print(json.dumps(p))' "$real_live_credentials")"
real_live_after_restart="$(curl -fsS -H 'content-type: application/json' \
  -d "$real_live_heartbeat" \
  "$real_base/api/v1/scheduler/action-runs/$real_live_action_id/heartbeat")"
[[ "$(json_field "$real_live_after_restart" toolLeaseStatus)" == active ]]
[[ "$(json_field "$real_live_after_restart" endpointRefs.0)" == "$real_live_endpoint" ]]

real_live_stop_request="$(new_uuid)"
real_live_stop_payload="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"renewalToken":sys.argv[2],
 "mode":"release","reason":"用户完成预览"},ensure_ascii=False))' \
  "$real_live_stop_request" "$real_live_renewal_token")"
real_live_stop="$(curl -fsS -H 'content-type: application/json' -d "$real_live_stop_payload" \
  "$real_base/api/v1/projects/$real_project_1/tool-leases/$real_live_lease_id/stop")"
[[ "$(json_field "$real_live_stop" result.action.status)" == cancellation_requested ]]
real_live_cancel_signal="$(curl -fsS -H 'content-type: application/json' \
  -d "$real_live_heartbeat" \
  "$real_base/api/v1/scheduler/action-runs/$real_live_action_id/heartbeat")"
[[ "$(json_field "$real_live_cancel_signal" cancellationRequested)" == true ]]
docker rm -f "$real_tool_live" >/dev/null
real_live_finish="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p.update({"retainedOutputs":[],
 "result":{"processExited":True,"endpointClosed":True}}); print(json.dumps(p))' \
  "$real_live_credentials")"
real_live_terminal="$(curl -fsS -H 'content-type: application/json' -d "$real_live_finish" \
  "$real_base/api/v1/scheduler/action-runs/$real_live_action_id/tool-lease/finish")"
[[ "$(json_field "$real_live_terminal" action.status)" == succeeded ]]
[[ "$(json_field "$real_live_terminal" lease.status)" == released ]]
[[ "$(json_field "$real_live_terminal" lease.cleanupStatus)" == succeeded ]]
real_live_stop_replay="$(curl -fsS -H 'content-type: application/json' -d "$real_live_stop_payload" \
  "$real_base/api/v1/projects/$real_project_1/tool-leases/$real_live_lease_id/stop")"
[[ "$(json_field "$real_live_stop_replay" replayed)" == true ]]
if curl -fsS --max-time 1 "http://127.0.0.1:$real_tool_live_port/health" >/dev/null 2>&1; then
  echo "released ToolLease endpoint remained reachable" >&2
  exit 1
fi

IFS='|' read -r real_project_3 real_branch_3 real_session_3 real_worktree_3 real_snapshot_3 \
  <<< "$(create_project_session '验证持续工具 launcher 崩溃清理' plugin-agent-3)"
real_snapshot_3="$(seed_file "$real_project_3" "$real_session_3" "$real_worktree_3" \
  "$real_snapshot_3" web/page.html "$(<"$real_repo_root/tests/fixtures/plugins/page.html")" seed-crash-page)"
bind_environment "$real_project_3" "$real_session_3" \
  "$(json_field "$real_environment_1" id)" >/dev/null
real_crash_payload="$(python3 -c 'import json,sys,uuid
m=json.loads(sys.argv[1]); print(json.dumps({"clientRequestId":str(uuid.uuid4()),
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"serve","input":{"sourcePath":"web/page.html"},
 "baseWorkspaceSnapshot":sys.argv[2],"softTtlSeconds":10,"durationSeconds":60}))' \
  "$real_playwright_manifest" "$real_snapshot_3")"
real_crash_lease="$(curl -fsS -H 'content-type: application/json' -d "$real_crash_payload" \
  "$real_base/api/v1/projects/$real_project_3/sessions/$real_session_3/tool-leases")"
real_crash_lease_id="$(json_field "$real_crash_lease" lease.id)"
real_crash_action_id="$(json_field "$real_crash_lease" action.id)"
real_crash_action_token="real_crash_action_token_0123456789abcdef"
real_crash_claim="$(claim_real_action "$real_tool_worker" "$real_tool_worker_token" \
  "$real_crash_action_token" 5)"
[[ "$(json_field "$real_crash_claim" action.id)" == "$real_crash_action_id" ]]
real_crash_credentials="$(real_lease_credentials "$real_crash_claim" "$real_tool_worker" \
  "$real_tool_worker_token" "$real_crash_action_token")"
launch_real_persistent_tool "$real_tool_crash" "$real_crash_claim" "$real_worktree_3"
real_tool_crash_port="$(wait_for_real_tool "$real_tool_crash")"
real_crash_endpoint="http://$real_tool_crash:4173"
real_crash_activate="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["endpointRefs"]=[sys.argv[2]]; print(json.dumps(p))' \
  "$real_crash_credentials" "$real_crash_endpoint")"
curl -fsS -H 'content-type: application/json' -d "$real_crash_activate" \
  "$real_base/api/v1/scheduler/action-runs/$real_crash_action_id/tool-lease/activate" >/dev/null
docker rm -f "$real_tool_crash" >/dev/null
sleep 6
real_crash_reconcile="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"workerId\":\"$real_reconcile_worker\",\"workerToken\":\"$real_reconcile_worker_token\",\"limit\":100}" \
  "$real_base/api/v1/scheduler/reconcile")"
[[ "$(json_field "$real_crash_reconcile" waitingActions)" == 1 ]]
real_crash_expired="$(curl -fsS \
  "$real_base/api/v1/projects/$real_project_3/tool-leases/$real_crash_lease_id")"
[[ "$(json_field "$real_crash_expired" status)" == expired ]]
[[ "$(json_field "$real_crash_expired" cleanupStatus)" == pending ]]
real_cleanup_payload="$(python3 -c 'import json,sys
print(json.dumps({"workerId":sys.argv[1],"workerToken":sys.argv[2],
 "status":"succeeded","summary":"launcher confirmed exact container is absent"}))' \
  "$real_reconcile_worker" "$real_reconcile_worker_token")"
real_crash_clean="$(curl -fsS -H 'content-type: application/json' -d "$real_cleanup_payload" \
  "$real_base/api/v1/scheduler/tool-leases/$real_crash_lease_id/cleanup")"
[[ "$(json_field "$real_crash_clean" cleanupStatus)" == succeeded ]]
real_crash_notifications="$(curl -fsS \
  "$real_base/api/v1/projects/$real_project_3/notifications")"
python3 -c 'import json,sys
n=json.loads(sys.argv[1])["notifications"]
assert len(n)==1 and n[0]["kind"]=="tool_lease_expired" and n[0]["status"]=="unread", n' \
  "$real_crash_notifications"
if curl -fsS --max-time 1 "http://127.0.0.1:$real_tool_crash_port/health" >/dev/null 2>&1; then
  echo "crashed ToolLease endpoint remained reachable" >&2
  exit 1
fi

real_ppt_draft="$(plugin_draft fudian.tools.pptmaster 1.0.0 'PPTMaster compatibility' \
  "sha256:$(printf 'a%.0s' {1..64})" build)"
real_ppt_manifest="$(curl -fsS -H 'content-type: application/json' -d "$real_ppt_draft" \
  "$real_base/api/v1/plugins")"
real_install_request_id="$(new_uuid)"
real_install_request_payload="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"pluginId":"fudian.tools.pptmaster",
 "versionRequirement":"1.0.0","capability":"presentation.build",
 "reason":"PPTMaster requires a licensed adapter and explicit user authorization"}))' \
  "$real_install_request_id")"
real_install_request="$(curl -fsS -H 'content-type: application/json' -d "$real_install_request_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/plugin-install-requests")"
real_install_request_replay="$(curl -fsS -H 'content-type: application/json' -d "$real_install_request_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/plugin-install-requests")"
[[ "$(json_field "$real_install_request" id)" == "$(json_field "$real_install_request_replay" id)" ]]
[[ "$(json_field "$real_install_request" status)" == requested ]]
real_ppt_environment_status="$(curl -sS -o "$real_tmp/ppt-environment.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"schemaVersion":1,
 "baseRuntime":{"kind":"fudian-runner","digest":sys.argv[1]},
 "plugins":[{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]}],
 "toolchains":{},"dependencyLocks":[],"targetPlatform":"linux-amd64","features":[],"buildParameters":{},
 "networkPolicy":"denied","resourcePolicy":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":128,"pids":64,"timeoutSeconds":60,"stdoutBytes":65536,"stderrBytes":65536},
 "environmentPolicy":{"allowedNames":[],"secretReferences":[]}}))' "$real_runner_digest" "$real_ppt_manifest")" \
  "$real_base/api/v1/environments")"
[[ "$real_ppt_environment_status" == 403 ]]
[[ "$(json_field "$(<"$real_tmp/ppt-environment.json")" code)" == plugin_not_installed ]]

real_playwright_install_id="$(json_field "$real_playwright_install" id)"
curl -fsS -H 'content-type: application/json' -d '{"reason":"revocation execution test"}' \
  "$real_base/api/v1/plugin-installations/$real_playwright_install_id/revoke" >/dev/null
real_revoked_payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"inspect","input":{"sourcePath":"web/page.html","screenshotPath":"reports/revoked.png","reportPath":"reports/revoked.json"},
 "baseWorkspaceSnapshot":sys.argv[3],"allowedWrites":["reports/**"],"deletePaths":[],"timeoutSeconds":60}))' \
  "$(new_uuid)" "$real_playwright_manifest" "$real_snapshot_1")"
real_revoked_status="$(curl -sS -o "$real_tmp/revoked-plugin.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_revoked_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-executions")"
[[ "$real_revoked_status" == 403 ]]
[[ "$(json_field "$(<"$real_tmp/revoked-plugin.json")" code)" == plugin_revoked ]]

real_alternate_python_draft="$(plugin_draft fudian.tools.python 1.0.0 'Conflicting Python package' \
  "sha256:$(printf 'b%.0s' {1..64})" run)"
curl -fsS -H 'content-type: application/json' -d "$real_alternate_python_draft" \
  "$real_base/api/v1/plugins" >/dev/null
real_conflict_status="$(curl -sS -o "$real_tmp/digest-conflict.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d '{"pluginId":"fudian.tools.python","version":"1.0.0"}' \
  "$real_base/api/v1/plugins/resolve")"
[[ "$real_conflict_status" == 409 ]]
[[ "$(json_field "$(<"$real_tmp/digest-conflict.json")" code)" == plugin_digest_conflict ]]

curl -fsS -H 'content-type: application/json' -d '{"reason":"publisher compromise test"}' \
  "$real_base/api/v1/plugin-publishers/fudian.local/revoke" >/dev/null
real_publisher_revoked_payload="$(python3 -c 'import json,sys
m=json.loads(sys.argv[2]); print(json.dumps({"clientRequestId":sys.argv[1],
 "plugin":{"pluginId":m["pluginId"],"version":m["version"],"contentDigest":m["contentDigest"]},
 "toolName":"check","input":{"sourcePath":"src/valid.rs","reportPath":"reports/revoked-rust.json"},
 "baseWorkspaceSnapshot":sys.argv[3],"allowedWrites":["reports/**"],"deletePaths":[],"timeoutSeconds":60}))' \
  "$(new_uuid)" "$real_rust_manifest" "$real_snapshot_1")"
real_publisher_revoked_status="$(curl -sS -o "$real_tmp/revoked-publisher.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$real_publisher_revoked_payload" \
  "$real_base/api/v1/projects/$real_project_1/sessions/$real_session_1/tool-executions")"
[[ "$real_publisher_revoked_status" == 403 ]]
[[ "$(json_field "$(<"$real_tmp/revoked-publisher.json")" code)" == plugin_revoked ]]

docker exec -i "$real_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test <<SQL >/dev/null
DO \$\$
BEGIN
  IF (SELECT count(*) FROM plugin_publishers) <> 1
     OR (SELECT count(*) FROM plugin_installations) <> 5
     OR (SELECT count(*) FROM tool_execution_requests) <> 5
     OR (SELECT count(*) FROM tool_calls WHERE runner_job_id IS NOT NULL) <> 5
     OR (SELECT count(*) FROM plugin_install_requests) <> 1
     OR (SELECT count(*) FROM runner_jobs WHERE status = 'succeeded') <> 11
     OR (SELECT count(*) FROM goal_contributions WHERE runner_job_id IS NOT NULL) <> 11
     OR (SELECT count(*) FROM tool_leases) <> 2
     OR (SELECT count(*) FROM tool_leases WHERE cleanup_status = 'succeeded') <> 2
     OR (SELECT status FROM tool_leases WHERE id = '$real_live_lease_id') <> 'released'
     OR (SELECT status FROM tool_leases WHERE id = '$real_crash_lease_id') <> 'expired'
     OR EXISTS (SELECT 1 FROM action_run_leases WHERE status = 'active')
     OR (SELECT status FROM goal_sessions WHERE id = '$real_session_3') <> 'exception_paused'
     OR (SELECT count(*) FROM goal_notifications WHERE project_id = '$real_project_3') <> 1
     OR EXISTS (SELECT 1 FROM workspace_write_leases WHERE status = 'active')
     OR EXISTS (
       SELECT 1 FROM tool_execution_requests execution
       JOIN runner_jobs job ON job.id = execution.runner_job_id
       JOIN tool_calls call ON call.runner_job_id = job.id
       WHERE execution.project_id <> job.project_id
          OR execution.session_id <> job.session_id
          OR execution.plugin_digest <> call.plugin_digest
          OR execution.environment_fingerprint <> call.environment_fingerprint
          OR call.result->'result'->>'resultWorkspaceSnapshot' IS NULL
     ) THEN
    RAISE EXCEPTION 'unexpected real plugin audit state';
  END IF;
END;
\$\$;
SQL

echo "real plugin HTTP flow passed: signed OCI, exact images, Git CAS, persistent ToolLease, Chromium and revocation"
