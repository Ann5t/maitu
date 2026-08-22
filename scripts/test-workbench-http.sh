#!/usr/bin/env bash
set -euo pipefail

workbench_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workbench_suffix="$$"
workbench_network="fudian-workbench-test-$workbench_suffix"
workbench_db="fudian-workbench-db-$workbench_suffix"
workbench_app="fudian-workbench-app-$workbench_suffix"
workbench_tmp="$(mktemp -d)"
workbench_worker_bootstrap="workbench_worker_bootstrap_0123456789abcdef"

# shellcheck source=scripts/review-worker-test-lib.sh
. "$workbench_repo_root/scripts/review-worker-test-lib.sh"

cleanup_workbench_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$workbench_app" >/dev/null 2>&1; then
    docker logs "$workbench_app" >&2 || true
  fi
  [[ "$workbench_app" == fudian-workbench-app-* ]] \
    && docker rm -f "$workbench_app" >/dev/null 2>&1 || true
  [[ "$workbench_db" == fudian-workbench-db-* ]] \
    && docker rm -f "$workbench_db" >/dev/null 2>&1 || true
  [[ "$workbench_network" == fudian-workbench-test-* ]] \
    && docker network rm "$workbench_network" >/dev/null 2>&1 || true
  [[ "$workbench_tmp" == /tmp/tmp.* ]] && rm -rf "$workbench_tmp"
  return "$exit_status"
}
trap cleanup_workbench_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

html_attribute() {
  python3 -c 'import html,re,sys
source=sys.stdin.read(); match=re.search(sys.argv[1]+r"=\"([^\"]+)\"",source)
assert match, "attribute not found: "+sys.argv[1]
print(html.unescape(match.group(1)))' "$1"
}

post_form() {
  local url="$1"
  shift
  curl -fsS -o /dev/null -D "$workbench_tmp/headers" "$@" "$url"
  [[ "$(awk 'NR==1 {print $2}' "$workbench_tmp/headers")" == 303 ]]
}

docker network create "$workbench_network" >/dev/null
docker run -d --name "$workbench_db" --network "$workbench_network" \
  --network-alias workbench-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for workbench_attempt in $(seq 1 30); do
  if docker exec "$workbench_db" pg_isready -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$workbench_attempt" == 30 ]] && docker logs "$workbench_db" && exit 1
  sleep 1
done

docker run -d --name "$workbench_app" --network "$workbench_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@workbench-db:5432/fudian_test \
  -e FUDIAN_BIND=0.0.0.0:3000 -e ARTIFACT_ROOT=/tmp/fudian-workbench-artifacts \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$workbench_worker_bootstrap" \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$workbench_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null
workbench_port="$(docker port "$workbench_app" 3000/tcp | sed -n 's/.*://p')"
workbench_base="http://127.0.0.1:$workbench_port"
for workbench_attempt in $(seq 1 60); do
  if curl -fsS "$workbench_base/api/health" >/dev/null 2>&1; then break; fi
  [[ "$workbench_attempt" == 60 ]] && docker logs "$workbench_app" && exit 1
  sleep 1
done

curl -fsS -o /dev/null -D "$workbench_tmp/create.headers" \
  --data-urlencode 'intent=<script>不能进入页面</script>：完成工作台闭环' \
  "$workbench_base/projects"
workbench_location="$(awk 'tolower($1)=="location:" {print $2}' "$workbench_tmp/create.headers" | tr -d '\r')"
workbench_project_id="$(python3 -c 'import re,sys; print(re.search(r"/projects/([0-9a-f-]+)",sys.argv[1]).group(1))' "$workbench_location")"
workbench_project_url="$workbench_base/projects/$workbench_project_id"
workbench_command_url="$workbench_project_url/goal-commands"

workbench_html="$(curl -fsS -D "$workbench_tmp/page.headers" "$workbench_project_url")"
grep -q 'id="goal-workbench"' <<< "$workbench_html"
grep -q 'data-projection-version="goal-lanes-v1"' <<< "$workbench_html"
grep -q '先形成第一条目标枝干' <<< "$workbench_html"
grep -q '&lt;script&gt;不能进入页面&lt;/script&gt;' <<< "$workbench_html"
! grep -q '<script>不能进入页面</script>' <<< "$workbench_html"
grep -qi '^content-security-policy:' "$workbench_tmp/page.headers"

post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" \
  --data-urlencode 'action=proposal.create' \
  --data-urlencode 'why_needed=建立可从手机审核的目标枝干工作台' \
  --data-urlencode 'desired_outcome=从表单创建目标，暂停恢复后完成拟合并审核' \
  --data-urlencode 'validation_plan=从浏览器运行结构化 HTML 表单闭环' \
  --data-urlencode 'stop_conditions=用户接受根枝干候选' \
  --data-urlencode 'unknowns=移动端最终视觉仍需用户凭感觉调整'
workbench_html="$(curl -fsS "$workbench_project_url?tab=goals")"
workbench_proposal_id="$(html_attribute data-proposal-id <<< "$workbench_html")"
grep -q '移动端最终视觉' <<< "$workbench_html"

post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=proposal.submit' \
  --data-urlencode "proposal_id=$workbench_proposal_id" --data-urlencode 'expected_revision=1'
post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=proposal.approve' \
  --data-urlencode "proposal_id=$workbench_proposal_id" --data-urlencode 'expected_revision=1' \
  --data-urlencode 'branch_name=工作台根目标' \
  --data-urlencode 'assignment=用结构化页面验证完整审核环' \
  --data-urlencode 'agent_identity=workbench-worker'
workbench_html="$(curl -fsS "$workbench_project_url?tab=goals")"
workbench_session_id="$(html_attribute data-session-id <<< "$workbench_html")"
grep -q 'FILES / ARTIFACTS' <<< "$workbench_html"
grep -q 'TOOLS / BROWSER / TESTS' <<< "$workbench_html"
grep -q 'data-input-upload' <<< "$workbench_html"

post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=session.request_judgment' \
  --data-urlencode "session_id=$workbench_session_id" \
  --data-urlencode 'question=这个紧凑工作台是否值得继续？' \
  --data-urlencode 'candidates=继续' --data-urlencode 'evidence=主要操作已在同页展示' \
  --data-urlencode 'recommendation=建议继续'
workbench_html="$(curl -fsS "$workbench_project_url?tab=goals&session=$workbench_session_id")"
grep -q '这个紧凑工作台是否值得继续' <<< "$workbench_html"
grep -q '现在可以显式恢复' <<< "$workbench_html"
post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=session.resume' \
  --data-urlencode "session_id=$workbench_session_id" \
  --data-urlencode 'resolution=用户同意继续这一轮实现'

post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=session.add_contribution' \
  --data-urlencode "session_id=$workbench_session_id" --data-urlencode 'contribution_kind=evidence' \
  --data-urlencode 'title=结构化工作台证据' \
  --data-urlencode 'body=根目标从 Proposal 到审核全部通过 HTML 表单推进'
workbench_html="$(curl -fsS "$workbench_project_url?tab=goals&session=$workbench_session_id")"
workbench_contribution_ids="$(python3 -c 'import html,re,sys
source=sys.stdin.read(); match=re.search(r"name=\"contribution_ids\" value=\"([^\"]+)\"",source)
assert match; print(html.unescape(match.group(1)))' <<< "$workbench_html")"
workbench_contract_id="$(python3 -c 'import html,re,sys
source=sys.stdin.read(); match=re.search(r"name=\"contract_version_id\" value=\"([^\"]+)\"",source)
assert match; print(html.unescape(match.group(1)))' <<< "$workbench_html")"

post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=merge.propose' \
  --data-urlencode "session_id=$workbench_session_id" \
  --data-urlencode "contract_version_id=$workbench_contract_id" \
  --data-urlencode "contribution_ids=$workbench_contribution_ids" \
  --data-urlencode 'test_evidence=HTML 表单暂停恢复通过' \
  --data-urlencode 'self_check=目标、验证和停止条件已逐项核对'
workbench_html="$(curl -fsS "$workbench_project_url?tab=goals&session=$workbench_session_id")"
workbench_gate_id="$(html_attribute data-review-gate-id <<< "$workbench_html")"
grep -q '候选现场已冻结' <<< "$workbench_html"

test_review_gate "$workbench_base" "$workbench_worker_bootstrap" \
  "$workbench_project_id" "$workbench_gate_id" recommend_accept \
  "契约、证据和暂停边界都已满足"
post_form "$workbench_command_url" \
  --data-urlencode "client_request_id=$(new_uuid)" --data-urlencode 'action=review.human_decide' \
  --data-urlencode "review_gate_id=$workbench_gate_id" --data-urlencode 'review_decision=accept' \
  --data-urlencode "selected_contribution_ids=$workbench_contribution_ids" \
  --data-urlencode 'rationale=用户确认这条根目标已完成'

workbench_html="$(curl -fsS "$workbench_project_url?tab=goals&session=$workbench_session_id")"
grep -q '用户确认这条根目标已完成' <<< "$workbench_html"
grep -q '你的最终决定' <<< "$workbench_html"
workbench_states="$(docker exec "$workbench_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT state FROM projects WHERE id='$workbench_project_id'),
          (SELECT status FROM goal_branches WHERE project_id='$workbench_project_id'),
          (SELECT status FROM goal_sessions WHERE id='$workbench_session_id'),
          (SELECT count(*) FROM goal_review_decisions WHERE project_id='$workbench_project_id');")"
[[ "$workbench_states" == 'completed|completed|accepted|2' ]]

echo "workbench HTML flow passed: structured Proposal, judgment pause/resume, contribution, AI review and human acceptance"
