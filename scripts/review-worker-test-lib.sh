#!/usr/bin/env bash

# Shared black-box helper for tests whose subject is not the Review Worker
# protocol itself.  The dedicated review/integration suite additionally proves
# read-only mounts, identity rejection, fencing, conflict and crash recovery.
test_review_gate() {
  local review_test_base="$1"
  local review_test_bootstrap="$2"
  local review_test_project="$3"
  local review_test_gate="$4"
  local review_test_decision="$5"
  local review_test_rationale="$6"
  local review_test_worker_id
  local review_test_worker_token
  local review_test_lease_token
  local review_test_claim
  local review_test_action
  local review_test_lease_id
  local review_test_fencing
  local review_test_report
  local review_test_body

  review_test_worker_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  review_test_worker_token="review_test_worker_${review_test_worker_id//-/}"
  review_test_lease_token="review_test_lease_${review_test_worker_id//-/}"
  curl -fsS -H 'content-type: application/json' \
    -H "x-fudian-worker-bootstrap: $review_test_bootstrap" \
    -d "$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"workerId":sys.argv[1],
 "workerToken":sys.argv[2],"displayName":"independent-test-reviewer-"+sys.argv[1],
 "capabilities":["review.goal_candidate.v1"]}))' \
      "$review_test_worker_id" "$review_test_worker_token")" \
    "$review_test_base/api/v1/scheduler/workers" >/dev/null

  for _ in $(seq 1 20); do
    review_test_claim="$(curl -fsS -H 'content-type: application/json' \
      -d "{\"workerId\":\"$review_test_worker_id\",\"workerToken\":\"$review_test_worker_token\",\"clientRequestId\":\"$(python3 -c 'import uuid; print(uuid.uuid4())')\",\"leaseToken\":\"$review_test_lease_token\",\"softTtlSeconds\":120,\"hardTtlSeconds\":300}" \
      "$review_test_base/api/v1/scheduler/claim")"
    if python3 -c 'import json,sys; raise SystemExit(json.loads(sys.argv[1]).get("action") is None)' \
      "$review_test_claim"; then
      break
    fi
    sleep 0.25
  done
  python3 -c 'import json,sys
c=json.loads(sys.argv[1]); a=c.get("action")
assert a and a["projectId"]==sys.argv[2] and a["subjectId"]==sys.argv[3]
assert a["kind"]=="review" and a["capability"]=="review.goal_candidate.v1"' \
    "$review_test_claim" "$review_test_project" "$review_test_gate"
  review_test_action="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["action"]["id"])' \
    "$review_test_claim")"
  review_test_lease_id="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["lease"]["id"])' \
    "$review_test_claim")"
  review_test_fencing="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["lease"]["fencingToken"])' \
    "$review_test_claim")"
  review_test_report="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1])["action"]["payload"]
print(json.dumps({"schemaVersion":1,"candidateDigest":p["candidateDigest"],
 "contractVersionId":p["contractVersionId"],"observedHeadCommit":p["headCommit"],
 "observedTreeId":p["treeId"],"observedWorkspaceSnapshot":p["workspaceSnapshot"],
 "environmentFingerprint":p["environmentFingerprint"],"decision":sys.argv[2],
 "rationale":sys.argv[3],"contractCheck":{"testContract":"checked"},
 "counterexamples":[],"retestEvidence":["隔离专项套件已验证同一协议的只读执行"],
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
    "$review_test_claim" "$review_test_decision" "$review_test_rationale")"
  review_test_body="$(python3 -c 'import json,sys
print(json.dumps({"workerId":sys.argv[1],"workerToken":sys.argv[2],"leaseId":sys.argv[3],
 "leaseToken":sys.argv[4],"fencingToken":int(sys.argv[5]),"result":json.loads(sys.argv[6])}))' \
    "$review_test_worker_id" "$review_test_worker_token" "$review_test_lease_id" \
    "$review_test_lease_token" "$review_test_fencing" "$review_test_report")"
  curl -fsS -H 'content-type: application/json' -d "$review_test_body" \
    "$review_test_base/api/v1/scheduler/action-runs/$review_test_action/complete" >/dev/null
}

test_integrate_goal() {
  local integration_test_base="$1"
  local integration_test_bootstrap="$2"
  local integration_test_project="$3"
  local integration_test_id="$4"
  local integration_test_worker_id
  local integration_test_worker_token
  local integration_test_lease_token
  local integration_test_claim
  local integration_test_action
  local integration_test_credentials
  local integration_test_prepared
  local integration_test_report
  local integration_test_body

  integration_test_worker_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  integration_test_worker_token="integration_test_worker_${integration_test_worker_id//-/}"
  integration_test_lease_token="integration_test_lease_${integration_test_worker_id//-/}"
  curl -fsS -H 'content-type: application/json' \
    -H "x-fudian-worker-bootstrap: $integration_test_bootstrap" \
    -d "$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"workerId":sys.argv[1],
 "workerToken":sys.argv[2],"displayName":"independent-test-integrator-"+sys.argv[1],
 "capabilities":["integration.goal_branch.v1"]}))' \
      "$integration_test_worker_id" "$integration_test_worker_token")" \
    "$integration_test_base/api/v1/scheduler/workers" >/dev/null

  for _ in $(seq 1 20); do
    integration_test_claim="$(curl -fsS -H 'content-type: application/json' \
      -d "{\"workerId\":\"$integration_test_worker_id\",\"workerToken\":\"$integration_test_worker_token\",\"clientRequestId\":\"$(python3 -c 'import uuid; print(uuid.uuid4())')\",\"leaseToken\":\"$integration_test_lease_token\",\"softTtlSeconds\":120,\"hardTtlSeconds\":300}" \
      "$integration_test_base/api/v1/scheduler/claim")"
    if python3 -c 'import json,sys; raise SystemExit(json.loads(sys.argv[1]).get("action") is None)' \
      "$integration_test_claim"; then
      break
    fi
    sleep 0.25
  done
  python3 -c 'import json,sys
c=json.loads(sys.argv[1]); a=c.get("action")
assert a and a["projectId"]==sys.argv[2] and a["subjectId"]==sys.argv[3]
assert a["kind"]=="integration" and a["capability"]=="integration.goal_branch.v1"' \
    "$integration_test_claim" "$integration_test_project" "$integration_test_id"
  integration_test_action="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["action"]["id"])' \
    "$integration_test_claim")"
  integration_test_credentials="$(python3 -c 'import json,sys
c=json.loads(sys.argv[1]); print(json.dumps({"workerId":sys.argv[2],"workerToken":sys.argv[3],
 "leaseId":c["lease"]["id"],"leaseToken":sys.argv[4],
 "fencingToken":c["lease"]["fencingToken"]}))' \
    "$integration_test_claim" "$integration_test_worker_id" "$integration_test_worker_token" \
    "$integration_test_lease_token")"
  integration_test_prepared="$(curl -fsS -H 'content-type: application/json' \
    -d "$integration_test_credentials" \
    "$integration_test_base/api/v1/scheduler/action-runs/$integration_test_action/integrations/$integration_test_id/prepare")"
  integration_test_report="$(python3 -c 'import json,sys
p=json.loads(sys.argv[1]); print(json.dumps({"schemaVersion":1,
 "candidateCommit":p["candidateCommit"],"candidateTreeId":p["candidateTreeId"],
 "candidateWorkspaceSnapshot":p["candidateWorkspaceSnapshot"],"status":"passed",
 "checks":["测试场景的父目标回归通过"],"contractCheck":{"testContract":"passed"},
 "isolation":{"candidateReadOnly":True,"noWorkspaceWrites":True,"noNewPrivileges":True,
 "dockerSocketAbsent":True,"hostSecretsAbsent":True,"effectiveCapabilitiesHex":"0000000000000000"}}))' \
    "$integration_test_prepared")"
  integration_test_body="$(python3 -c 'import json,sys
b=json.loads(sys.argv[1]); b["validation"]=json.loads(sys.argv[2]); print(json.dumps(b))' \
    "$integration_test_credentials" "$integration_test_report")"
  curl -fsS -H 'content-type: application/json' -d "$integration_test_body" \
    "$integration_test_base/api/v1/scheduler/action-runs/$integration_test_action/integrations/$integration_test_id/finalize"
}
