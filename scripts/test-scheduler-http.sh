#!/usr/bin/env bash
set -euo pipefail

scheduler_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scheduler_suffix="$$"
scheduler_network="fudian-scheduler-test-$scheduler_suffix"
scheduler_db="fudian-scheduler-db-$scheduler_suffix"
scheduler_app="fudian-scheduler-app-$scheduler_suffix"
scheduler_tmp="$(mktemp -d)"
scheduler_bootstrap="bootstrap_scheduler_test_0123456789abcdef"

cleanup_scheduler_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$scheduler_app" >/dev/null 2>&1; then
    docker logs "$scheduler_app" >&2 || true
  fi
  [[ "$scheduler_app" == fudian-scheduler-app-* ]] \
    && docker rm -f "$scheduler_app" >/dev/null 2>&1 || true
  [[ "$scheduler_db" == fudian-scheduler-db-* ]] \
    && docker rm -f "$scheduler_db" >/dev/null 2>&1 || true
  [[ "$scheduler_network" == fudian-scheduler-test-* ]] \
    && docker network rm "$scheduler_network" >/dev/null 2>&1 || true
  [[ "$scheduler_tmp" == /tmp/tmp.* && -d "$scheduler_tmp" ]] \
    && rm -rf -- "$scheduler_tmp"
  return "$exit_status"
}
trap cleanup_scheduler_stack EXIT

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

wait_for_scheduler_app() {
  scheduler_port="$(docker port "$scheduler_app" 3000/tcp | sed -n 's/.*://p')"
  scheduler_base="http://127.0.0.1:$scheduler_port"
  for scheduler_attempt in $(seq 1 60); do
    if curl -fsS "$scheduler_base/api/health" >/dev/null 2>&1; then
      return 0
    fi
    if [[ "$scheduler_attempt" == 60 ]]; then
      docker logs "$scheduler_app"
      return 1
    fi
    sleep 1
  done
}

start_scheduler_app() {
  local bootstrap="${1:-}"
  local bootstrap_args=()
  if [[ -n "$bootstrap" ]]; then
    bootstrap_args=(-e "FUDIAN_WORKER_BOOTSTRAP_TOKEN=$bootstrap")
  fi
  docker run -d --name "$scheduler_app" --network "$scheduler_network" \
    -p 127.0.0.1::3000 \
    -e DATABASE_URL=postgres://fudian_test:fudian_test_only@scheduler-db:5432/fudian_test \
    -e FUDIAN_SECURITY_MODE=disabled \
    -e FUDIAN_BIND=0.0.0.0:3000 \
    -e ARTIFACT_ROOT=/data/artifacts \
    -e REPOSITORY_ROOT=/data/repositories \
    -e WORKTREE_ROOT=/data/worktrees \
    -e RUNNER_OUTPUT_ROOT=/data/runner \
    -e RUNNER_RUNTIME_DIGEST="$scheduler_runner_digest" \
    -e RUST_LOG=fudian=info \
    "${bootstrap_args[@]}" \
    --mount "type=bind,src=$scheduler_repo_root,dst=/app" \
    --mount "type=bind,src=$scheduler_tmp/artifacts,dst=/data/artifacts" \
    --mount "type=bind,src=$scheduler_tmp/repositories,dst=/data/repositories" \
    --mount "type=bind,src=$scheduler_tmp/worktrees,dst=/data/worktrees" \
    --mount "type=bind,src=$scheduler_tmp/runner,dst=/data/runner" \
    --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
    --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
    --mount type=volume,src=fudian_rust_target,dst=/app/target \
    fudian-nextgen-app:latest cargo run --locked >/dev/null
  wait_for_scheduler_app
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
    "$scheduler_base/api/v1/projects/$project_id/goal-commands"
}

create_project_session() {
  local intent="$1"
  local project
  local project_id
  local proposal
  local proposal_id
  local approval
  project="$(curl -fsS -H 'content-type: application/json' \
    -d "{\"intent\":\"$intent\"}" "$scheduler_base/api/projects")"
  project_id="$(json_field "$project" id)"
  proposal="$(post_goal_for "$project_id" proposal.create "{\"revision\":$scheduler_revision}")"
  proposal_id="$(json_field "$proposal" result.proposalId)"
  post_goal_for "$project_id" proposal.submit \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1}" >/dev/null
  approval="$(post_goal_for "$project_id" proposal.approve \
    "{\"proposalId\":\"$proposal_id\",\"expectedRevision\":1,\"branchName\":\"调度恢复目标\",\"assignment\":\"验证持久 ActionRun\",\"agentIdentity\":\"scheduler-test-agent\"}")"
  printf '%s|%s|%s' "$project_id" \
    "$(json_field "$approval" result.goalBranchId)" \
    "$(json_field "$approval" result.sessionId)"
}

register_worker() {
  local worker_id="$1"
  local worker_token="$2"
  local request_id="$3"
  local capabilities="$4"
  local payload
  payload="$(python3 -c 'import json,sys
print(json.dumps({"clientRequestId":sys.argv[1],"workerId":sys.argv[2],
 "workerToken":sys.argv[3],"displayName":"isolated scheduler worker",
 "capabilities":json.loads(sys.argv[4])}))' \
    "$request_id" "$worker_id" "$worker_token" "$capabilities")"
  curl -fsS -H 'content-type: application/json' \
    -H "x-fudian-worker-bootstrap: $scheduler_bootstrap" \
    -d "$payload" "$scheduler_base/api/v1/scheduler/workers"
}

enqueue_action() {
  local project_id="$1"
  local session_id="$2"
  local request_id="$3"
  local safety="$4"
  local payload_json="$5"
  local max_attempts="${6:-3}"
  local deadline="${7:-}"
  local body
  body="$(python3 -c 'import json,sys
body={"clientRequestId":sys.argv[1],"kind":"agent_step","capability":"agent.execute",
 "payload":json.loads(sys.argv[2]),"retrySafety":sys.argv[3],"maxAttempts":int(sys.argv[4])}
if sys.argv[5]: body["deadlineAt"]=sys.argv[5]
print(json.dumps(body))' "$request_id" "$payload_json" "$safety" "$max_attempts" "$deadline")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$scheduler_base/api/v1/projects/$project_id/sessions/$session_id/action-runs"
}

claim_action() {
  local worker_id="$1"
  local worker_token="$2"
  local request_id="$3"
  local lease_token="$4"
  local soft_ttl="${5:-2}"
  local hard_ttl="${6:-30}"
  curl -fsS -H 'content-type: application/json' \
    -d "{\"workerId\":\"$worker_id\",\"workerToken\":\"$worker_token\",\"clientRequestId\":\"$request_id\",\"leaseToken\":\"$lease_token\",\"softTtlSeconds\":$soft_ttl,\"hardTtlSeconds\":$hard_ttl}" \
    "$scheduler_base/api/v1/scheduler/claim"
}

lease_credentials() {
  local claim="$1"
  local worker_id="$2"
  local worker_token="$3"
  local lease_token="$4"
  python3 -c 'import json,sys
c=json.loads(sys.argv[1]); print(json.dumps({"workerId":sys.argv[2],"workerToken":sys.argv[3],
 "leaseId":c["lease"]["id"],"leaseToken":sys.argv[4],"fencingToken":c["lease"]["fencingToken"]}))' \
    "$claim" "$worker_id" "$worker_token" "$lease_token"
}

reconcile_actions() {
  curl -fsS -H 'content-type: application/json' \
    -d "{\"workerId\":\"$scheduler_reconcile_worker\",\"workerToken\":\"$scheduler_reconcile_token\",\"limit\":100}" \
    "$scheduler_base/api/v1/scheduler/reconcile"
}

mkdir -p "$scheduler_tmp/artifacts" "$scheduler_tmp/repositories" \
  "$scheduler_tmp/worktrees" "$scheduler_tmp/runner"
chmod 0777 "$scheduler_tmp/artifacts" "$scheduler_tmp/repositories" \
  "$scheduler_tmp/worktrees" "$scheduler_tmp/runner"

docker network create "$scheduler_network" >/dev/null
docker run -d --name "$scheduler_db" --network "$scheduler_network" \
  --network-alias scheduler-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for scheduler_attempt in $(seq 1 30); do
  if docker exec "$scheduler_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$scheduler_attempt" == 30 ]] && docker logs "$scheduler_db" && exit 1
  sleep 1
done

scheduler_runner_digest="$(docker run --rm --entrypoint /usr/local/bin/fudian-runner \
  fudian-nextgen-runner:latest digest)"

start_scheduler_app
scheduler_disabled_payload="$(python3 -c 'import json,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"workerId":str(uuid.uuid4()),
 "workerToken":"disabled_worker_token_0123456789abcdef","displayName":"disabled worker",
 "capabilities":["agent.execute"]}))')"
scheduler_disabled_status="$(curl -sS -o "$scheduler_tmp/disabled.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$scheduler_disabled_payload" \
  "$scheduler_base/api/v1/scheduler/workers")"
[[ "$scheduler_disabled_status" == 403 ]]
[[ "$(json_field "$(<"$scheduler_tmp/disabled.json")" code)" == worker_registration_disabled ]]
docker rm -f "$scheduler_app" >/dev/null

start_scheduler_app "$scheduler_bootstrap"

scheduler_bad_status="$(curl -sS -o "$scheduler_tmp/bad-bootstrap.json" -w '%{http_code}' \
  -H 'content-type: application/json' -H 'x-fudian-worker-bootstrap: wrong_bootstrap_token_0123456789abcdef' \
  -d "$scheduler_disabled_payload" "$scheduler_base/api/v1/scheduler/workers")"
[[ "$scheduler_bad_status" == 403 ]]
[[ "$(json_field "$(<"$scheduler_tmp/bad-bootstrap.json")" code)" == invalid_worker_bootstrap ]]

scheduler_worker_1="$(new_uuid)"
scheduler_worker_2="$(new_uuid)"
scheduler_reconcile_worker="$(new_uuid)"
scheduler_worker_1_token="agent_worker_one_token_0123456789abcdef"
scheduler_worker_2_token="agent_worker_two_token_0123456789abcdef"
scheduler_reconcile_token="reconcile_worker_token_0123456789abcdef"
scheduler_worker_1_request="$(new_uuid)"
scheduler_worker_1_registration="$(register_worker "$scheduler_worker_1" \
  "$scheduler_worker_1_token" "$scheduler_worker_1_request" '["agent.execute"]')"
[[ "$(json_field "$scheduler_worker_1_registration" replayed)" == false ]]
scheduler_worker_1_replay="$(register_worker "$scheduler_worker_1" \
  "$scheduler_worker_1_token" "$scheduler_worker_1_request" '["agent.execute"]')"
[[ "$(json_field "$scheduler_worker_1_replay" replayed)" == true ]]
register_worker "$scheduler_worker_2" "$scheduler_worker_2_token" \
  "$(new_uuid)" '["agent.execute"]' >/dev/null
register_worker "$scheduler_reconcile_worker" "$scheduler_reconcile_token" \
  "$(new_uuid)" '["scheduler.reconcile"]' >/dev/null

scheduler_revision='{
  "whyNeeded":"验证持久调度与崩溃恢复",
  "contract":{
    "desiredOutcome":"后台行动在重启和 Worker 失联后可安全恢复",
    "hardConstraints":["fencing","未知副作用不自动重放"],
    "subjectivePreferences":[],"unknowns":[],"nonGoals":["不访问生产"],
    "validationPlan":["并发领取、重启、超时和取消"],"judgmentTriggers":[],
    "stopConditions":["恢复矩阵通过"],"expectedContributions":["调度证据"]
  },
  "expectedContributions":["ActionRun 审计"],"explorationPlan":[],"contextInheritance":{},
  "toolRequirements":[],
  "capabilityPolicy":{
    "network":"denied","networkDestinations":[],"externalWrites":[],"accountReferences":[],
    "paidOperations":false,"deployment":false,
    "readScopes":["current_worktree","parent_snapshot"],"writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":128,"pids":64,"timeoutSeconds":120,"stdoutBytes":65536,"stderrBytes":65536}
  },
  "inferences":[],"revisionReason":null
}'

IFS='|' read -r scheduler_project scheduler_branch scheduler_session \
  <<< "$(create_project_session '验证持久调度恢复')"

scheduler_safe_request="$(new_uuid)"
scheduler_safe="$(enqueue_action "$scheduler_project" "$scheduler_session" \
  "$scheduler_safe_request" safe '{"step":"safe-restart"}')"
scheduler_safe_action="$(json_field "$scheduler_safe" action.id)"
scheduler_safe_replay="$(enqueue_action "$scheduler_project" "$scheduler_session" \
  "$scheduler_safe_request" safe '{"step":"safe-restart"}')"
[[ "$(json_field "$scheduler_safe_replay" replayed)" == true ]]
scheduler_conflict_status="$(curl -sS -o "$scheduler_tmp/enqueue-conflict.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$scheduler_safe_request\",\"kind\":\"agent_step\",\"capability\":\"agent.execute\",\"payload\":{\"step\":\"different\"},\"retrySafety\":\"safe\"}" \
  "$scheduler_base/api/v1/projects/$scheduler_project/sessions/$scheduler_session/action-runs")"
[[ "$scheduler_conflict_status" == 409 ]]
[[ "$(json_field "$(<"$scheduler_tmp/enqueue-conflict.json")" code)" == idempotency_conflict ]]

scheduler_claim_1_request="$(new_uuid)"
scheduler_claim_2_request="$(new_uuid)"
scheduler_claim_1_token="action_lease_one_token_0123456789abcdef"
scheduler_claim_2_token="action_lease_two_token_0123456789abcdef"
claim_action "$scheduler_worker_1" "$scheduler_worker_1_token" \
  "$scheduler_claim_1_request" "$scheduler_claim_1_token" >"$scheduler_tmp/claim-1.json" &
scheduler_claim_pid_1=$!
claim_action "$scheduler_worker_2" "$scheduler_worker_2_token" \
  "$scheduler_claim_2_request" "$scheduler_claim_2_token" >"$scheduler_tmp/claim-2.json" &
scheduler_claim_pid_2=$!
wait "$scheduler_claim_pid_1"
wait "$scheduler_claim_pid_2"
scheduler_claim_1="$(<"$scheduler_tmp/claim-1.json")"
scheduler_claim_2="$(<"$scheduler_tmp/claim-2.json")"
python3 -c 'import json,sys
responses=[json.loads(sys.argv[1]),json.loads(sys.argv[2])]
assert sum(response["action"] is not None for response in responses)==1, responses' \
  "$scheduler_claim_1" "$scheduler_claim_2"
if [[ "$(json_field "$scheduler_claim_1" action)" != null ]]; then
  scheduler_winner_worker="$scheduler_worker_1"
  scheduler_winner_token="$scheduler_worker_1_token"
  scheduler_winner_claim_request="$scheduler_claim_1_request"
  scheduler_winner_lease_token="$scheduler_claim_1_token"
  scheduler_winner_claim="$scheduler_claim_1"
  scheduler_loser_worker="$scheduler_worker_2"
  scheduler_loser_token="$scheduler_worker_2_token"
else
  scheduler_winner_worker="$scheduler_worker_2"
  scheduler_winner_token="$scheduler_worker_2_token"
  scheduler_winner_claim_request="$scheduler_claim_2_request"
  scheduler_winner_lease_token="$scheduler_claim_2_token"
  scheduler_winner_claim="$scheduler_claim_2"
  scheduler_loser_worker="$scheduler_worker_1"
  scheduler_loser_token="$scheduler_worker_1_token"
fi
[[ "$(json_field "$scheduler_winner_claim" action.id)" == "$scheduler_safe_action" ]]
scheduler_claim_replay="$(claim_action "$scheduler_winner_worker" "$scheduler_winner_token" \
  "$scheduler_winner_claim_request" "$scheduler_winner_lease_token")"
[[ "$(json_field "$scheduler_claim_replay" replayed)" == true ]]
[[ "$(json_field "$scheduler_claim_replay" lease.id)" == "$(json_field "$scheduler_winner_claim" lease.id)" ]]

scheduler_credentials="$(lease_credentials "$scheduler_winner_claim" "$scheduler_winner_worker" \
  "$scheduler_winner_token" "$scheduler_winner_lease_token")"
scheduler_heartbeat_payload="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["extendSeconds"]=5; print(json.dumps(p))' "$scheduler_credentials")"
curl -fsS -H 'content-type: application/json' -d "$scheduler_heartbeat_payload" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_safe_action/heartbeat" >/dev/null

docker restart "$scheduler_app" >/dev/null
wait_for_scheduler_app
scheduler_after_restart="$(curl -fsS -H 'content-type: application/json' \
  -d "$scheduler_heartbeat_payload" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_safe_action/heartbeat")"
[[ "$(json_field "$scheduler_after_restart" status)" == running ]]
sleep 6
scheduler_reconcile="$(reconcile_actions)"
[[ "$(json_field "$scheduler_reconcile" requeuedActions)" == 1 ]]
scheduler_requeued="$(curl -fsS \
  "$scheduler_base/api/v1/projects/$scheduler_project/action-runs/$scheduler_safe_action")"
[[ "$(json_field "$scheduler_requeued" status)" == queued ]]
scheduler_stale_payload="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["result"]={"late":True}; print(json.dumps(p))' "$scheduler_credentials")"
scheduler_stale_status="$(curl -sS -o "$scheduler_tmp/stale-fence.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$scheduler_stale_payload" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_safe_action/complete")"
[[ "$scheduler_stale_status" == 403 || "$scheduler_stale_status" == 409 ]]

sleep 2
scheduler_reclaim_request="$(new_uuid)"
scheduler_reclaim_token="action_reclaim_token_0123456789abcdef"
scheduler_reclaim="$(claim_action "$scheduler_loser_worker" "$scheduler_loser_token" \
  "$scheduler_reclaim_request" "$scheduler_reclaim_token")"
[[ "$(json_field "$scheduler_reclaim" action.id)" == "$scheduler_safe_action" ]]
[[ "$(json_field "$scheduler_reclaim" lease.fencingToken)" == 2 ]]
scheduler_reclaim_credentials="$(lease_credentials "$scheduler_reclaim" "$scheduler_loser_worker" \
  "$scheduler_loser_token" "$scheduler_reclaim_token")"
scheduler_complete_payload="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["result"]={"recovered":True}; print(json.dumps(p))' \
  "$scheduler_reclaim_credentials")"
scheduler_completed="$(curl -fsS -H 'content-type: application/json' -d "$scheduler_complete_payload" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_safe_action/complete")"
[[ "$(json_field "$scheduler_completed" status)" == succeeded ]]

scheduler_unknown="$(enqueue_action "$scheduler_project" "$scheduler_session" \
  "$(new_uuid)" unknown '{"step":"unknown-side-effect"}')"
scheduler_unknown_action="$(json_field "$scheduler_unknown" action.id)"
scheduler_unknown_claim_token="unknown_claim_token_0123456789abcdef"
scheduler_unknown_claim="$(claim_action "$scheduler_worker_1" "$scheduler_worker_1_token" \
  "$(new_uuid)" "$scheduler_unknown_claim_token")"
[[ "$(json_field "$scheduler_unknown_claim" action.id)" == "$scheduler_unknown_action" ]]
sleep 3
scheduler_unknown_reconcile="$(reconcile_actions)"
[[ "$(json_field "$scheduler_unknown_reconcile" waitingActions)" == 1 ]]
reconcile_actions >/dev/null
scheduler_unknown_record="$(curl -fsS \
  "$scheduler_base/api/v1/projects/$scheduler_project/action-runs/$scheduler_unknown_action")"
[[ "$(json_field "$scheduler_unknown_record" status)" == waiting ]]
scheduler_notifications="$(curl -fsS \
  "$scheduler_base/api/v1/projects/$scheduler_project/notifications")"
python3 -c 'import json,sys
n=json.loads(sys.argv[1])["notifications"]
assert len(n)==1 and n[0]["kind"]=="action_waiting" and n[0]["status"]=="unread", n' \
  "$scheduler_notifications"
scheduler_notification_id="$(json_field "$scheduler_notifications" notifications.0.id)"
scheduler_read_request="$(new_uuid)"
scheduler_read="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$scheduler_read_request\"}" \
  "$scheduler_base/api/v1/projects/$scheduler_project/notifications/$scheduler_notification_id/read")"
[[ "$(json_field "$scheduler_read" notification.status)" == read ]]
scheduler_read_replay="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$scheduler_read_request\"}" \
  "$scheduler_base/api/v1/projects/$scheduler_project/notifications/$scheduler_notification_id/read")"
[[ "$(json_field "$scheduler_read_replay" replayed)" == true ]]

scheduler_resolve_request="$(new_uuid)"
scheduler_resolve="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$scheduler_resolve_request\",\"decision\":\"retry\",\"reason\":\"人工确认没有外部副作用\"}" \
  "$scheduler_base/api/v1/projects/$scheduler_project/action-runs/$scheduler_unknown_action/resolve")"
[[ "$(json_field "$scheduler_resolve" action.status)" == queued ]]
scheduler_unknown_retry_token="unknown_retry_token_0123456789abcdef"
scheduler_unknown_retry="$(claim_action "$scheduler_worker_2" "$scheduler_worker_2_token" \
  "$(new_uuid)" "$scheduler_unknown_retry_token")"
scheduler_unknown_retry_credentials="$(lease_credentials "$scheduler_unknown_retry" \
  "$scheduler_worker_2" "$scheduler_worker_2_token" "$scheduler_unknown_retry_token")"
scheduler_unknown_complete="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["result"]={"humanApprovedRetry":True}; print(json.dumps(p))' \
  "$scheduler_unknown_retry_credentials")"
curl -fsS -H 'content-type: application/json' -d "$scheduler_unknown_complete" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_unknown_action/complete" >/dev/null

scheduler_cancel="$(enqueue_action "$scheduler_project" "$scheduler_session" \
  "$(new_uuid)" safe '{"step":"cancel-race"}')"
scheduler_cancel_action="$(json_field "$scheduler_cancel" action.id)"
scheduler_cancel_claim_token="cancel_claim_token_0123456789abcdef"
scheduler_cancel_claim="$(claim_action "$scheduler_worker_1" "$scheduler_worker_1_token" \
  "$(new_uuid)" "$scheduler_cancel_claim_token" 10 30)"
scheduler_cancel_credentials="$(lease_credentials "$scheduler_cancel_claim" \
  "$scheduler_worker_1" "$scheduler_worker_1_token" "$scheduler_cancel_claim_token")"
scheduler_cancel_request="$(new_uuid)"
scheduler_cancelled_request="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$scheduler_cancel_request\",\"reason\":\"用户先取消\"}" \
  "$scheduler_base/api/v1/projects/$scheduler_project/action-runs/$scheduler_cancel_action/cancel")"
[[ "$(json_field "$scheduler_cancelled_request" action.status)" == cancellation_requested ]]
scheduler_cancel_heartbeat="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["extendSeconds"]=10; print(json.dumps(p))' "$scheduler_cancel_credentials")"
scheduler_cancel_signal="$(curl -fsS -H 'content-type: application/json' \
  -d "$scheduler_cancel_heartbeat" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_cancel_action/heartbeat")"
[[ "$(json_field "$scheduler_cancel_signal" cancellationRequested)" == true ]]
scheduler_illegal_complete="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p["result"]={"ignoredCancel":True}; print(json.dumps(p))' \
  "$scheduler_cancel_credentials")"
scheduler_illegal_complete_status="$(curl -sS -o "$scheduler_tmp/illegal-complete.json" -w '%{http_code}' \
  -H 'content-type: application/json' -d "$scheduler_illegal_complete" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_cancel_action/complete")"
[[ "$scheduler_illegal_complete_status" == 409 ]]
[[ "$(json_field "$(<"$scheduler_tmp/illegal-complete.json")" code)" == action_cancellation_requested ]]
scheduler_cancel_ack="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); p.update({"failureKind":"cancelled","summary":"子进程已经终止","detail":{"processExited":True}}); print(json.dumps(p))' \
  "$scheduler_cancel_credentials")"
scheduler_cancel_terminal="$(curl -fsS -H 'content-type: application/json' -d "$scheduler_cancel_ack" \
  "$scheduler_base/api/v1/scheduler/action-runs/$scheduler_cancel_action/fail")"
[[ "$(json_field "$scheduler_cancel_terminal" status)" == cancelled ]]

scheduler_empty="$(claim_action "$scheduler_worker_1" "$scheduler_worker_1_token" \
  "$(new_uuid)" empty_claim_token_0123456789abcdef)"
[[ "$(json_field "$scheduler_empty" action)" == null ]]
[[ "$(json_field "$scheduler_empty" retryAfterSeconds)" == 2 ]]

IFS='|' read -r scheduler_deadline_project scheduler_deadline_branch scheduler_deadline_session \
  <<< "$(create_project_session '验证排队截止时间')"
scheduler_deadline="$(python3 -c 'from datetime import datetime,timedelta,timezone
print((datetime.now(timezone.utc)+timedelta(seconds=3)).isoformat().replace("+00:00","Z"))')"
scheduler_deadline_action_response="$(enqueue_action "$scheduler_deadline_project" \
  "$scheduler_deadline_session" "$(new_uuid)" safe '{"step":"deadline"}' 1 "$scheduler_deadline")"
scheduler_deadline_action="$(json_field "$scheduler_deadline_action_response" action.id)"
sleep 4
scheduler_deadline_reconcile="$(reconcile_actions)"
[[ "$(json_field "$scheduler_deadline_reconcile" deadlineFailures)" == 1 ]]
scheduler_deadline_record="$(curl -fsS \
  "$scheduler_base/api/v1/projects/$scheduler_deadline_project/action-runs/$scheduler_deadline_action")"
[[ "$(json_field "$scheduler_deadline_record" status)" == failed ]]

docker exec -i "$scheduler_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test <<SQL >/dev/null
DO \$\$
BEGIN
  IF (SELECT count(*) FROM scheduler_workers) <> 3
     OR EXISTS (SELECT 1 FROM action_run_leases WHERE status = 'active')
     OR (SELECT attempt_count FROM goal_action_runs WHERE id = '$scheduler_safe_action') <> 2
     OR (SELECT count(*) FROM goal_notifications) <> 2
     OR (SELECT count(*) FROM notification_outbox WHERE status = 'suppressed' AND adapter = 'none') <> 2
     OR (SELECT count(*) FROM goal_attention_items WHERE kind = 'action_run_failure') <> 2
     OR EXISTS (SELECT 1 FROM scheduler_workers WHERE token_digest IN (
       '$scheduler_worker_1_token', '$scheduler_worker_2_token', '$scheduler_reconcile_token'))
     OR (SELECT status FROM goal_sessions WHERE id = '$scheduler_session') <> 'running'
     OR (SELECT status FROM goal_sessions WHERE id = '$scheduler_deadline_session') <> 'exception_paused'
     OR NOT EXISTS (
       SELECT 1 FROM goal_action_events WHERE action_run_id = '$scheduler_safe_action'
       AND event_type = 'action.requeued') THEN
    RAISE EXCEPTION 'unexpected scheduler audit state';
  END IF;
END;
\$\$;
SQL

echo "scheduler HTTP flow passed: closed registration, concurrent claim, restart, fencing, recovery, notification and cancel"
