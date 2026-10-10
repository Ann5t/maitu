#!/usr/bin/env bash
set -euo pipefail

security_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
security_suffix="$$"
security_network="fudian-security-test-$security_suffix"
security_db="fudian-security-db-$security_suffix"
security_app="fudian-security-app-$security_suffix"
security_caddy="fudian-security-caddy-$security_suffix"
security_tool_small="fudian-security-tool-small-$security_suffix"
security_tool_large="fudian-security-tool-large-$security_suffix"
security_tmp="$(mktemp -d)"
security_host="security.test"
security_port="$((20000 + security_suffix % 20000))"
security_origin="https://$security_host:$security_port"
security_setup_token="setup_token_security_test_0123456789abcdef"
security_pepper="pepper_security_test_0123456789abcdef"
security_worker_token="worker_security_test_0123456789abcdef"
security_password="Initial-passphrase-2026"
security_new_password="Recovered-passphrase-2026"
security_runner_image="${SECURITY_RUNNER_IMAGE:-fudian-nextgen-runner:latest}"
security_browser_image="${PLAYWRIGHT_IMAGE:-mcr.microsoft.com/playwright:v1.62.0-noble}"
security_screenshot_dir="${SCREENSHOT_DIR:-$security_repo_root/docs/assets/screenshots}"

# shellcheck source=scripts/docker-test-lib.sh
. "$security_repo_root/scripts/docker-test-lib.sh"

cleanup_security_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$security_app" >/dev/null 2>&1; then
    docker logs "$security_app" >&2 || true
  fi
  for container in "$security_app" "$security_caddy" "$security_tool_small" \
    "$security_tool_large" "$security_db"; do
    if [[ "$container" == fudian-security-* ]]; then
      docker rm -fv "$container" >/dev/null 2>&1 || true
    fi
  done
  if [[ "$security_network" == fudian-security-test-* ]]; then
    docker network rm "$security_network" >/dev/null 2>&1 || true
  fi
  if [[ "$security_tmp" == /tmp/tmp.* && -d "$security_tmp" ]]; then
    fudian_test_remove_bind_tree "$security_tmp"
  fi
  return "$exit_status"
}
trap cleanup_security_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys
value=json.loads(sys.argv[1])
for key in sys.argv[2].split("."):
    value=value[int(key)] if isinstance(value,list) else value[key]
print(value)' "$1" "$2"
}

csrf_from_jar() {
  awk '$6 == "__Host-fudian_csrf" { value=$7 } END { print value }' "$1"
}

session_from_jar() {
  awk '$6 == "__Host-fudian_session" { value=$7 } END { print value }' "$1"
}

secure_curl() {
  curl --silent --show-error --insecure --noproxy '*' \
    --resolve "$security_host:$security_port:127.0.0.1" "$@"
}

authenticated_json() {
  local jar="$1"
  local method="$2"
  local path="$3"
  local body="${4:-}"
  local csrf
  local request_args=()
  csrf="$(csrf_from_jar "$jar")"
  if [[ -n "$body" ]]; then
    request_args=(--data "$body")
  fi
  secure_curl -b "$jar" -c "$jar" -X "$method" \
    -H "Origin: $security_origin" -H "x-csrf-token: $csrf" \
    -H 'content-type: application/json' "${request_args[@]}" \
    "$security_origin$path"
}

post_goal() {
  local jar="$1"
  local project_id="$2"
  local action="$3"
  local payload="$4"
  local request
  request="$(python3 -c 'import json,sys,uuid
print(json.dumps({"clientRequestId":str(uuid.uuid4()),"action":sys.argv[1],"payload":json.loads(sys.argv[2])},ensure_ascii=False))' "$action" "$payload")"
  authenticated_json "$jar" POST "/api/v1/projects/$project_id/goal-commands" "$request"
}

mkdir -p "$security_tmp/artifacts" "$security_tmp/repositories" \
  "$security_tmp/worktrees" "$security_tmp/runner" "$security_tmp/tool-small" \
  "$security_tmp/tool-large" "$security_tmp/caddy-data" "$security_tmp/caddy-config" \
  "$security_screenshot_dir"
chmod 0777 "$security_tmp/artifacts" "$security_tmp/repositories" \
  "$security_tmp/worktrees" "$security_tmp/runner" "$security_tmp/caddy-data" \
  "$security_tmp/caddy-config"
printf '<!doctype html><title>isolated ToolLease preview</title><p>broker-only preview</p>\n' \
  >"$security_tmp/tool-small/index.html"
dd if=/dev/zero of="$security_tmp/tool-large/large.bin" bs=1024 count=2 status=none

docker image inspect "$security_runner_image" >/dev/null
docker image inspect "$security_browser_image" >/dev/null
docker network create "$security_network" >/dev/null
security_network_cidr="$(docker network inspect -f '{{(index .IPAM.Config 0).Subnet}}' "$security_network")"

docker run -d --name "$security_db" --network "$security_network" \
  --network-alias security-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for security_attempt in $(seq 1 30); do
  if docker exec "$security_db" pg_isready -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$security_attempt" == 30 ]] && docker logs "$security_db" && exit 1
  sleep 1
done

docker run -d --name "$security_tool_small" --network "$security_network" \
  -e FUDIAN_INPUT=/workspace/input \
  --mount "type=bind,src=$security_tmp/tool-small,dst=/workspace/input,readonly" \
  --entrypoint /opt/fudian/fudian-tool-runtime "$security_runner_image" \
  serve-static index.html 4173 >/dev/null
docker run -d --name "$security_tool_large" --network "$security_network" \
  -e FUDIAN_INPUT=/workspace/input \
  --mount "type=bind,src=$security_tmp/tool-large,dst=/workspace/input,readonly" \
  --entrypoint /opt/fudian/fudian-tool-runtime "$security_runner_image" \
  serve-static large.bin 4173 >/dev/null
security_tool_small_ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$security_tool_small")"
security_tool_large_ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$security_tool_large")"

docker run -d --name "$security_caddy" --network "$security_network" \
  --network-alias "$security_host" \
  -p "127.0.0.1:$security_port:$security_port" \
  -e FUDIAN_SITE_HOST="$security_host" -e FUDIAN_SITE_PORT="$security_port" \
  --mount "type=bind,src=$security_repo_root/deploy/Caddyfile.test,dst=/etc/caddy/Caddyfile,readonly" \
  --mount "type=bind,src=$security_tmp/caddy-data,dst=/data" \
  --mount "type=bind,src=$security_tmp/caddy-config,dst=/config" \
  caddy:2.11.4-alpine@sha256:5f5c8640aae01df9654968d946d8f1a56c497f1dd5c5cda4cf95ab7c14d58648 >/dev/null
security_caddy_ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$security_caddy")"

docker run -d --name "$security_app" --network "$security_network" \
  --network-alias security-app \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@security-db:5432/fudian_test \
  -e FUDIAN_SECURITY_MODE=required \
  -e FUDIAN_PUBLIC_ORIGIN="$security_origin" \
  -e FUDIAN_AUTH_PEPPER="$security_pepper" \
  -e FUDIAN_SETUP_TOKEN="$security_setup_token" \
  -e FUDIAN_TRUSTED_PROXY_CIDRS="$security_caddy_ip/32" \
  -e FUDIAN_TOOL_PROXY_ALLOWED_CIDRS="$security_network_cidr" \
  -e FUDIAN_SESSION_TTL_SECONDS=3600 \
  -e FUDIAN_SESSION_IDLE_SECONDS=30 \
  -e FUDIAN_SESSION_ROTATION_SECONDS=1 \
  -e FUDIAN_LOGIN_ATTEMPTS=4 \
  -e FUDIAN_LOGIN_WINDOW_SECONDS=300 \
  -e FUDIAN_MUTATION_ATTEMPTS=300 \
  -e FUDIAN_HIGH_COST_ATTEMPTS=100 \
  -e FUDIAN_TOOL_PROXY_BODY_MAX_BYTES=512 \
  -e FUDIAN_TOOL_PROXY_RESPONSE_MAX_BYTES=512 \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$security_worker_token" \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT=/data/artifacts \
  -e REPOSITORY_ROOT=/data/repositories \
  -e WORKTREE_ROOT=/data/worktrees \
  -e RUNNER_OUTPUT_ROOT=/data/runner \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$security_repo_root,dst=/app" \
  --mount "type=bind,src=$security_tmp/artifacts,dst=/data/artifacts" \
  --mount "type=bind,src=$security_tmp/repositories,dst=/data/repositories" \
  --mount "type=bind,src=$security_tmp/worktrees,dst=/data/worktrees" \
  --mount "type=bind,src=$security_tmp/runner,dst=/data/runner" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run --locked >/dev/null

for security_attempt in $(seq 1 90); do
  if secure_curl --fail "$security_origin/api/health" >/dev/null 2>&1; then
    break
  fi
  [[ "$security_attempt" == 90 ]] && docker logs "$security_app" && docker logs "$security_caddy" && exit 1
  sleep 1
done

# A non-proxy peer cannot make forged Forwarded headers authoritative.
untrusted_output="$(docker run --rm --network "$security_network" \
  postgres:17-alpine sh -ec "wget -q -S -O /dev/null --header='X-Forwarded-Proto: https' --header='X-Forwarded-Host: $security_host:$security_port' http://security-app:3000/ 2>&1" \
  || true)"
untrusted_status="$(awk '$1 ~ /^HTTP\// { code=$2 } END { print code }' <<<"$untrusted_output")"
[[ "$untrusted_status" == 403 ]]

# Two simultaneous setup requests race on the singleton row; exactly one wins.
for setup_index in 1 2; do
  setup_username="owner$setup_index"
  (
    secure_curl -D "$security_tmp/setup-$setup_index.headers" \
      -c "$security_tmp/setup-$setup_index.jar" \
      -o "$security_tmp/setup-$setup_index.body" -w '%{http_code}' \
      -H "Origin: $security_origin" \
      --data-urlencode "username=$setup_username" \
      --data-urlencode "password=$security_password" \
      --data-urlencode "password_confirm=$security_password" \
      --data-urlencode "setup_token=$security_setup_token" \
      "$security_origin/auth/setup" >"$security_tmp/setup-$setup_index.status"
  ) &
done
wait
setup_status_1="$(<"$security_tmp/setup-1.status")"
setup_status_2="$(<"$security_tmp/setup-2.status")"
if [[ "$setup_status_1" == 200 && "$setup_status_2" == 409 ]]; then
  security_username=owner1
  security_jar="$security_tmp/setup-1.jar"
  security_setup_body="$security_tmp/setup-1.body"
  security_setup_headers="$security_tmp/setup-1.headers"
elif [[ "$setup_status_1" == 409 && "$setup_status_2" == 200 ]]; then
  security_username=owner2
  security_jar="$security_tmp/setup-2.jar"
  security_setup_body="$security_tmp/setup-2.body"
  security_setup_headers="$security_tmp/setup-2.headers"
else
  echo "并发 setup 结果异常：$setup_status_1/$setup_status_2" >&2
  exit 1
fi
security_recovery_code="$(grep -Eo '[0-9A-F]{8}(-[0-9A-F]{8}){4}' "$security_setup_body" | head -1)"
[[ -n "$security_recovery_code" ]]
grep -qi 'strict-transport-security: max-age=31536000' "$security_setup_headers"
grep -qi 'content-security-policy:' "$security_setup_headers"
grep -qi '__Host-fudian_session=.*Secure; HttpOnly; SameSite=Strict' "$security_setup_headers"
grep -qi '__Host-fudian_csrf=.*Secure; SameSite=Strict' "$security_setup_headers"

unauth_status="$(secure_curl -o /dev/null -w '%{http_code}' -H 'Accept: text/html' "$security_origin/")"
[[ "$unauth_status" == 307 ]]
secure_curl --fail -b "$security_jar" -c "$security_jar" "$security_origin/" \
  >"$security_tmp/authenticated.html"
grep -q 'id="maitu-project-create"' "$security_tmp/authenticated.html"

session_before="$(session_from_jar "$security_jar")"
sleep 2
secure_curl --fail -b "$security_jar" -c "$security_jar" "$security_origin/auth/status" \
  >"$security_tmp/status.json"
session_after="$(session_from_jar "$security_jar")"
[[ -n "$session_before" && -n "$session_after" && "$session_before" != "$session_after" ]]

wrong_origin_status="$(secure_curl -b "$security_jar" -o "$security_tmp/wrong-origin.json" \
  -w '%{http_code}' -H 'Origin: https://evil.invalid' -H 'content-type: application/json' \
  -d '{"intent":"must not exist"}' "$security_origin/api/projects")"
[[ "$wrong_origin_status" == 403 ]]
missing_csrf_status="$(secure_curl -b "$security_jar" -o "$security_tmp/missing-csrf.json" \
  -w '%{http_code}' -H "Origin: $security_origin" -H 'content-type: application/json' \
  -d '{"intent":"must not exist"}' "$security_origin/api/projects")"
[[ "$missing_csrf_status" == 403 ]]

security_project_json="$(authenticated_json "$security_jar" POST /api/projects '{"intent":"隔离 HTTPS 安全验收项目"}')"
security_project_id="$(json_field "$security_project_json" id)"
security_revision='{
  "whyNeeded":"验证受认证 ToolLease 代理",
  "contract":{
    "desiredOutcome":"内部持续工具只通过同源受控入口访问",
    "hardConstraints":["不公开工具端口"],"subjectivePreferences":[],"unknowns":[],
    "nonGoals":["不访问生产"],"validationPlan":["HTTPS 与 SSRF 边界"],
    "judgmentTriggers":[],"stopConditions":["安全矩阵通过"],
    "expectedContributions":["安全证据"]
  },
  "expectedContributions":["安全证据"],"explorationPlan":[],"contextInheritance":{},
  "toolRequirements":[],
  "capabilityPolicy":{
    "network":"denied","networkDestinations":[],"externalWrites":[],"accountReferences":[],
    "paidOperations":false,"deployment":false,"readScopes":["current_worktree"],
    "writePaths":["**"],
    "maximumResources":{"cpuMillis":1000,"memoryMiB":512,"diskMiB":128,"pids":64,
      "timeoutSeconds":120,"stdoutBytes":65536,"stderrBytes":65536}
  },
  "inferences":[],"revisionReason":null
}'
security_proposal="$(post_goal "$security_jar" "$security_project_id" proposal.create \
  "{\"revision\":$security_revision}")"
security_proposal_id="$(json_field "$security_proposal" result.proposalId)"
post_goal "$security_jar" "$security_project_id" proposal.submit \
  "{\"proposalId\":\"$security_proposal_id\",\"expectedRevision\":1}" >/dev/null
security_approval="$(post_goal "$security_jar" "$security_project_id" proposal.approve \
  "{\"proposalId\":\"$security_proposal_id\",\"expectedRevision\":1,\"branchName\":\"安全代理目标\",\"assignment\":\"验证同源工具入口\",\"agentIdentity\":\"security-test-agent\"}")"
security_branch_id="$(json_field "$security_approval" result.goalBranchId)"
security_session_id="$(json_field "$security_approval" result.sessionId)"
security_lease_id="$(new_uuid)"
security_expired_lease_id="$(new_uuid)"
security_digest="sha256:$(printf '1%.0s' $(seq 1 64))"
docker exec "$security_db" psql -v ON_ERROR_STOP=1 -U fudian_test -d fudian_test \
  -c "INSERT INTO tool_leases
    (id,project_id,goal_branch_id,session_id,plugin_id,plugin_version,plugin_digest,tool_name,
     environment_fingerprint,base_workspace_snapshot,status,renewal_token_digest,resource_policy,
     endpoint_refs,soft_expires_at,hard_expires_at,cleanup_status,last_heartbeat_at)
    VALUES
    ('$security_lease_id','$security_project_id','$security_branch_id','$security_session_id',
     'fudian.test.preview','1.0.0','$security_digest','serve','$security_digest','$security_digest',
     'active','$security_digest','{}',
     '[\"http://$security_tool_small_ip:4173\",\"http://$security_tool_large_ip:4173\",\"http://8.8.8.8:80\"]',
     now()+interval '10 minutes',now()+interval '20 minutes','pending',now()),
    ('$security_expired_lease_id','$security_project_id','$security_branch_id','$security_session_id',
     'fudian.test.preview','1.0.0','$security_digest','serve','$security_digest','$security_digest',
     'released','$security_digest','{}','[\"http://$security_tool_small_ip:4173\"]',
     now()+interval '10 minutes',now()+interval '20 minutes','succeeded',now());" >/dev/null

secure_curl --fail -b "$security_jar" -c "$security_jar" \
  -H 'Authorization: Bearer browser-secret-must-strip' \
  "$security_origin/api/v1/tool-leases/$security_lease_id/proxy/0" \
  >"$security_tmp/proxy-small.html"
grep -q 'broker-only preview' "$security_tmp/proxy-small.html"
proxy_large_status="$(secure_curl -b "$security_jar" -c "$security_jar" -o /dev/null -w '%{http_code}' \
  "$security_origin/api/v1/tool-leases/$security_lease_id/proxy/1")"
[[ "$proxy_large_status" == 502 ]]
proxy_external_status="$(secure_curl -b "$security_jar" -c "$security_jar" -o /dev/null -w '%{http_code}' \
  "$security_origin/api/v1/tool-leases/$security_lease_id/proxy/2")"
[[ "$proxy_external_status" == 403 ]]
proxy_expired_status="$(secure_curl -b "$security_jar" -c "$security_jar" -o /dev/null -w '%{http_code}' \
  "$security_origin/api/v1/tool-leases/$security_expired_lease_id/proxy/0")"
[[ "$proxy_expired_status" == 409 ]]
proxy_body_status="$(secure_curl -b "$security_jar" -c "$security_jar" -o /dev/null -w '%{http_code}' \
  -X POST -H "Origin: $security_origin" -H "x-csrf-token: $(csrf_from_jar "$security_jar")" \
  -H 'content-type: application/octet-stream' --data-binary @"$security_tmp/tool-large/large.bin" \
  "$security_origin/api/v1/tool-leases/$security_lease_id/proxy/0")"
[[ "$proxy_body_status" == 422 ]]

# Force the current browser session over the configured idle boundary, then recover it once.
cp "$security_jar" "$security_tmp/expired-session.jar"
docker exec "$security_db" psql -U fudian_test -d fudian_test \
  -c "UPDATE auth_sessions SET last_seen_at=now()-interval '1 hour' WHERE revoked_at IS NULL" >/dev/null
idle_status="$(secure_curl -b "$security_tmp/expired-session.jar" -o /dev/null -w '%{http_code}' \
  -H 'Accept: text/html' "$security_origin/")"
[[ "$idle_status" == 307 ]]
recovery_status="$(secure_curl -D "$security_tmp/recovery.headers" -c "$security_tmp/recovery.jar" \
  -o "$security_tmp/recovery.body" -w '%{http_code}' -H "Origin: $security_origin" \
  --data-urlencode "username=$security_username" \
  --data-urlencode "recovery_code=$security_recovery_code" \
  --data-urlencode "new_password=$security_new_password" \
  --data-urlencode "password_confirm=$security_new_password" \
  "$security_origin/auth/recover")"
[[ "$recovery_status" == 200 ]]
old_recovery_status="$(secure_curl -o /dev/null -w '%{http_code}' -H "Origin: $security_origin" \
  --data-urlencode "username=$security_username" \
  --data-urlencode "recovery_code=$security_recovery_code" \
  --data-urlencode "new_password=Another-passphrase-2026" \
  --data-urlencode "password_confirm=Another-passphrase-2026" \
  "$security_origin/auth/recover")"
[[ "$old_recovery_status" == 403 ]]
old_password_status="$(secure_curl -o /dev/null -w '%{http_code}' -H "Origin: $security_origin" \
  --data-urlencode "username=$security_username" --data-urlencode "password=$security_password" \
  "$security_origin/auth/login")"
[[ "$old_password_status" == 403 ]]

docker run --rm --init --ipc=host --network "$security_network" \
  -e BASE_URL="$security_origin" \
  -e FUDIAN_TEST_USERNAME="$security_username" \
  -e FUDIAN_TEST_PASSWORD="$security_new_password" \
  -e FUDIAN_TEST_PROJECT_ID="$security_project_id" \
  -e SCREENSHOT_DIR=/screenshots \
  --mount "type=bind,src=$security_repo_root,dst=/work,readonly" \
  --mount "type=bind,src=$security_screenshot_dir,dst=/screenshots" \
  --mount type=volume,src=fudian_playwright_npm_cache,dst=/root/.npm \
  "$security_browser_image" sh -c '
    npm install --prefix /tmp/pw --no-audit --no-fund @playwright/test@1.62.0 >/dev/null &&
    cp /work/tests/browser/security.spec.js /tmp/pw/security.spec.js &&
    cd /tmp/pw && ./node_modules/.bin/playwright test security.spec.js --reporter=line --workers=1
  '

for screenshot in private-desktop.png private-tablet.png private-mobile.png; do
  test -s "$security_screenshot_dir/$screenshot"
done

# Login throttling is database-backed and remains active after the application process restarts.
rate_status=0
for rate_attempt in $(seq 1 6); do
  rate_status="$(secure_curl -o /dev/null -w '%{http_code}' -H "Origin: $security_origin" \
    --data-urlencode 'username=unknown-user' --data-urlencode 'password=Wrong-passphrase-2026' \
    "$security_origin/auth/login")"
  [[ "$rate_status" == 429 ]] && break
done
[[ "$rate_status" == 429 ]]
docker restart "$security_app" >/dev/null
for security_attempt in $(seq 1 90); do
  if secure_curl --fail "$security_origin/api/health" >/dev/null 2>&1; then break; fi
  [[ "$security_attempt" == 90 ]] && docker logs "$security_app" && exit 1
  sleep 1
done
persisted_rate_status="$(secure_curl -o /dev/null -w '%{http_code}' -H "Origin: $security_origin" \
  --data-urlencode 'username=unknown-user' --data-urlencode 'password=Wrong-passphrase-2026' \
  "$security_origin/auth/login")"
[[ "$persisted_rate_status" == 429 ]]

secret_audit_count="$(docker exec "$security_db" psql -U fudian_test -d fudian_test -Atc \
  "SELECT count(*) FROM security_audit_events e WHERE row_to_json(e)::text LIKE '%$security_setup_token%'
   OR row_to_json(e)::text LIKE '%$security_password%' OR row_to_json(e)::text LIKE '%$security_recovery_code%';")"
security_migrations="$(docker exec "$security_db" psql -U fudian_test -d fudian_test -Atc \
  'SELECT count(*) FROM schema_migrations;')"
[[ "$secret_audit_count" == 0 ]]
[[ "$security_migrations" == 19 ]]

echo "security HTTPS passed: singleton setup, sessions, CSRF/origin, persistent limits, recovery, ToolLease proxy and Chromium 1440/820/390"
