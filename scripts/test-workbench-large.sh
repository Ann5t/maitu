#!/usr/bin/env bash
set -euo pipefail

large_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
large_suffix="$$"
large_network="fudian-workbench-large-test-$large_suffix"
large_db="fudian-workbench-large-db-$large_suffix"
large_app="fudian-workbench-large-app-$large_suffix"
large_tmp="$(mktemp -d)"
large_browser_image="mcr.microsoft.com/playwright:v1.62.0-noble"
large_screenshot_dir="${SCREENSHOT_DIR:-$large_tmp/screenshots}"

cleanup_large_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$large_app" >/dev/null 2>&1; then
    docker logs "$large_app" >&2 || true
  fi
  [[ "$large_app" == fudian-workbench-large-app-* ]] \
    && docker rm -f "$large_app" >/dev/null 2>&1 || true
  [[ "$large_db" == fudian-workbench-large-db-* ]] \
    && docker rm -f "$large_db" >/dev/null 2>&1 || true
  [[ "$large_network" == fudian-workbench-large-test-* ]] \
    && docker network rm "$large_network" >/dev/null 2>&1 || true
  [[ "$large_tmp" == /tmp/tmp.* ]] && rm -rf "$large_tmp"
  return "$exit_status"
}
trap cleanup_large_stack EXIT

docker image inspect "$large_browser_image" >/dev/null
mkdir -p "$large_screenshot_dir"
large_screenshot_dir="$(cd "$large_screenshot_dir" && pwd)"
docker network create "$large_network" >/dev/null
docker run -d --name "$large_db" --network "$large_network" \
  --network-alias large-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for large_attempt in $(seq 1 30); do
  if docker exec "$large_db" pg_isready -U fudian_test -d fudian_test >/dev/null 2>&1; then break; fi
  [[ "$large_attempt" == 30 ]] && docker logs "$large_db" && exit 1
  sleep 1
done

docker run -d --name "$large_app" --network "$large_network" \
  --network-alias large-app \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@large-db:5432/fudian_test \
  -e FUDIAN_BIND=0.0.0.0:3000 -e ARTIFACT_ROOT=/tmp/fudian-large-artifacts \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN=large_worker_bootstrap_0123456789abcdef \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$large_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null
large_port="$(docker port "$large_app" 3000/tcp | sed -n 's/.*://p')"
large_base="http://127.0.0.1:$large_port"
for large_attempt in $(seq 1 60); do
  if curl -fsS "$large_base/api/health" >/dev/null 2>&1; then break; fi
  [[ "$large_attempt" == 60 ]] && docker logs "$large_app" && exit 1
  sleep 1
done

curl -fsS -o /dev/null -D "$large_tmp/create.headers" \
  --data-urlencode 'intent=验证大型目标图的服务端与浏览器预算' \
  "$large_base/projects"
large_location="$(awk 'tolower($1)=="location:" {print $2}' "$large_tmp/create.headers" | tr -d '\r')"
large_project_id="$(python3 -c 'import re,sys; print(re.search(r"/projects/([0-9a-f-]+)",sys.argv[1]).group(1))' "$large_location")"

docker exec -i "$large_db" psql -v ON_ERROR_STOP=1 -v project_id="$large_project_id" \
  -U fudian_test -d fudian_test <<'SQL' >/dev/null
BEGIN;
SET CONSTRAINTS ALL DEFERRED;
CREATE TEMP TABLE large_seed_project(project_id uuid NOT NULL);
INSERT INTO large_seed_project VALUES (:'project_id');

DO $seed$
DECLARE
  v_project uuid := (SELECT project_id FROM large_seed_project);
  v_proposal uuid;
  v_branch uuid;
  v_contract uuid;
  v_session_1 uuid;
  v_session_2 uuid;
  v_session_3 uuid;
  v_parent_index integer;
  v_parent_branch uuid;
  v_parent_session uuid;
  v_branch_status text;
  v_head_status text;
  v_action uuid;
  v_attention uuid;
  v_gate uuid;
  v_integration uuid;
  branch_ids uuid[] := ARRAY[]::uuid[];
  head_session_ids uuid[] := ARRAY[]::uuid[];
BEGIN
  UPDATE projects SET state = 'active', current_focus = '大型目标图交互预算' WHERE id = v_project;

  FOR i IN 0..99 LOOP
    v_proposal := gen_random_uuid();
    v_branch := gen_random_uuid();
    v_contract := gen_random_uuid();
    v_session_1 := gen_random_uuid();
    v_session_2 := gen_random_uuid();
    v_session_3 := gen_random_uuid();
    IF i = 0 THEN
      v_parent_branch := NULL;
      v_parent_session := NULL;
    ELSE
      v_parent_index := ((i - 1) / 3) + 1;
      v_parent_branch := branch_ids[v_parent_index];
      v_parent_session := head_session_ids[v_parent_index];
    END IF;
    v_branch_status := CASE
      WHEN i > 0 AND i % 10 = 0 THEN 'waiting'
      WHEN i > 0 AND i % 10 = 1 THEN 'review_pending'
      WHEN i > 0 AND i % 10 = 2 THEN 'completed'
      WHEN i > 0 AND i % 10 = 3 THEN 'stopped'
      ELSE 'active'
    END;
    v_head_status := CASE v_branch_status
      WHEN 'waiting' THEN 'waiting_judgment'
      WHEN 'review_pending' THEN 'awaiting_merge_review'
      WHEN 'completed' THEN 'accepted'
      WHEN 'stopped' THEN 'stopped'
      ELSE 'running'
    END;

    INSERT INTO goal_branch_proposals
      (id, project_id, parent_goal_branch_id, parent_session_id, status,
       current_revision, approved_revision, approved_goal_branch_id, created_by, decided_at)
    VALUES
      (v_proposal, v_project, v_parent_branch, v_parent_session, 'approved',
       1, 1, v_branch, 'human', now());
    INSERT INTO goal_branch_proposal_revisions
      (id, proposal_id, revision, why_needed, contract, expected_contributions,
       exploration_plan, context_inheritance, tool_requirements, inferences,
       capability_policy, created_by)
    VALUES
      (gen_random_uuid(), v_proposal, 1, format('大型图目标 %s 的独立探索边界', i),
       jsonb_build_object(
         'desiredOutcome', format('目标 %s 在大型项目中保持可追踪、可审核', lpad(i::text, 3, '0')),
         'hardConstraints', jsonb_build_array('一条枝干只有一个写者'),
         'subjectivePreferences', '[]'::jsonb,
         'unknowns', CASE WHEN i % 10 = 0 THEN jsonb_build_array('等待用户判断方向') ELSE '[]'::jsonb END,
         'nonGoals', '[]'::jsonb,
         'validationPlan', jsonb_build_array('筛选与键盘定位正确'),
         'judgmentTriggers', jsonb_build_array('出现方向分歧时暂停'),
         'stopConditions', jsonb_build_array('状态与证据可审计'),
         'expectedContributions', jsonb_build_array('可回流结论'),
         'exploration', jsonb_build_object('mode','delivery','budgets','[]'::jsonb,'candidateOutputs','[]'::jsonb,'uncertaintyReduction','[]'::jsonb)),
       jsonb_build_array('大型图验收记录'), '[]'::jsonb, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
       '{}'::jsonb, 'human');
    INSERT INTO goal_branches
      (id, project_id, creating_proposal_id, parent_goal_branch_id,
       inherited_from_session_id, name, status, current_contract_version_id,
       head_session_id, git_branch_name, completed_at, stopped_at)
    VALUES
      (v_branch, v_project, v_proposal, v_parent_branch, v_parent_session,
       format('目标 %s · 大型图验收', lpad(i::text, 3, '0')), v_branch_status,
       v_contract, v_session_3, format('goal/%s', v_branch),
       CASE WHEN v_branch_status = 'completed' THEN now() END,
       CASE WHEN v_branch_status = 'stopped' THEN now() END);
    INSERT INTO goal_contract_versions
      (id, project_id, goal_branch_id, version, desired_outcome, hard_constraints,
       subjective_preferences, unknowns, non_goals, validation_plan, judgment_triggers,
       stop_conditions, expected_contributions, exploration_policy, source_proposal_id, created_by)
    VALUES
      (v_contract, v_project, v_branch, 1,
       format('目标 %s 在大型项目中保持可追踪、可审核', lpad(i::text, 3, '0')),
       jsonb_build_array('一条枝干只有一个写者'), '[]'::jsonb, '[]'::jsonb, '[]'::jsonb,
       jsonb_build_array('筛选与键盘定位正确'), jsonb_build_array('出现方向分歧时暂停'),
       jsonb_build_array('状态与证据可审计'), jsonb_build_array('可回流结论'),
       '{"mode":"delivery","budgets":[],"candidateOutputs":[],"uncertaintyReduction":[]}'::jsonb,
       v_proposal, 'human');
    INSERT INTO goal_sessions
      (id, project_id, goal_branch_id, session_number, status, assignment,
       agent_identity, contract_version_id, inherited_context, ended_at)
    VALUES
      (v_session_1, v_project, v_branch, 1, 'accepted',
       format('目标 %s：收集客观条件', lpad(i::text, 3, '0')), format('agent-%s-a', i),
       v_contract, '{}'::jsonb, now()),
      (v_session_2, v_project, v_branch, 2, 'accepted',
       format('目标 %s：实现并验证候选', lpad(i::text, 3, '0')), format('agent-%s-b', i),
       v_contract, '{}'::jsonb, now()),
      (v_session_3, v_project, v_branch, 3, v_head_status,
       format('目标 %s：整理证据并准备回流', lpad(i::text, 3, '0')), format('agent-%s-c', i),
       v_contract, '{}'::jsonb,
       CASE WHEN v_head_status IN ('accepted','stopped') THEN now() END);

    branch_ids := array_append(branch_ids, v_branch);
    head_session_ids := array_append(head_session_ids, v_session_3);

    IF v_branch_status = 'waiting' THEN
      v_attention := gen_random_uuid();
      INSERT INTO goal_attention_items
        (id, project_id, goal_branch_id, session_id, kind, dedupe_key, title, reason,
         safe_checkpoint, attempted, risk, user_action, recommendation)
      VALUES
        (v_attention, v_project, v_branch, v_session_3, 'judgment', format('large:%s:judgment', i),
         format('目标 %s 等待方向判断', lpad(i::text, 3, '0')),
         '两个候选都有可行证据，需要用户凭目标感受选择', '代码与证据已经提交到安全点',
         '已比较两个实现候选', '继续会扩大无效工作', '选择候选 A 或 B', '建议先审查候选 A');
    END IF;

    IF i > 0 AND i % 7 = 0 THEN
      v_action := gen_random_uuid();
      INSERT INTO goal_action_runs
        (id, project_id, goal_branch_id, session_id, client_request_id, request_hash,
         kind, capability, subject_kind, payload, retry_safety, status, max_attempts)
      VALUES
        (v_action, v_project, v_branch, v_session_3, gen_random_uuid(),
         'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
         'agent_step', 'agent.step.v1', 'none', jsonb_build_object('seed', i), 'safe',
         CASE WHEN i % 14 = 0 THEN 'waiting' ELSE 'queued' END, 3);
      INSERT INTO goal_action_events
        (id, project_id, action_run_id, event_type, actor_type, detail)
      VALUES (gen_random_uuid(), v_project, v_action, 'action.queued', 'system', jsonb_build_object('seed', i));
      IF i % 14 = 0 THEN
        INSERT INTO goal_notifications
          (id, project_id, goal_branch_id, session_id, action_run_id, dedupe_key,
           kind, severity, title, summary)
        VALUES
          (gen_random_uuid(), v_project, v_branch, v_session_3, v_action,
           format('large:%s:action-waiting', i), 'action_waiting', 'warning',
           format('目标 %s 的后台行动等待处理', lpad(i::text, 3, '0')),
           '行动已停在安全点，可从工作现场查看下一步');
      END IF;
    END IF;

    IF v_branch_status = 'review_pending' OR v_branch_status = 'completed' THEN
      v_gate := gen_random_uuid();
      INSERT INTO goal_review_gates
        (id, project_id, goal_branch_id, session_id, contract_version_id, status,
         candidate_snapshot, candidate_hash, test_evidence, risks, self_check)
      VALUES
        (v_gate, v_project, v_branch, v_session_3, v_contract, 'pending_ai_review',
         jsonb_build_object('seed', i),
         'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
         jsonb_build_array('大型图隔离候选'), '[]'::jsonb,
         jsonb_build_object('summary', '目标契约逐项自查通过'));
      IF v_branch_status = 'completed' THEN
        INSERT INTO goal_review_decisions
          (id, project_id, review_gate_id, actor_role, actor_identity, decision,
           rationale, contract_check, retest_evidence)
        VALUES
          (gen_random_uuid(), v_project, v_gate, 'review_ai', 'large-seed-reviewer',
           'recommend_accept', '隔离复验通过', jsonb_build_object('seed', true),
           jsonb_build_array('大型图候选复验'));
        UPDATE goal_review_gates SET status = 'pending_human_review' WHERE id = v_gate;
        INSERT INTO goal_review_decisions
          (id, project_id, review_gate_id, actor_role, decision, rationale)
        VALUES
          (gen_random_uuid(), v_project, v_gate, 'human', 'accept', '大型图种子中的已接受候选');
        UPDATE goal_review_gates SET status = 'accepted', resolved_at = now() WHERE id = v_gate;
        v_integration := gen_random_uuid();
        INSERT INTO goal_integrations
          (id, project_id, source_goal_branch_id, target_goal_branch_id,
           review_gate_id, kind, summary, git_integration_status)
        VALUES
          (v_integration, v_project, v_branch, v_parent_branch, v_gate, 'full',
           '用户已接受，物理 Git 集成尚未开始', 'not_attempted');
      END IF;
    END IF;
  END LOOP;

  FOR i IN 1..20 LOOP
    v_proposal := gen_random_uuid();
    v_parent_index := (i * 4) + 1;
    INSERT INTO goal_branch_proposals
      (id, project_id, parent_goal_branch_id, parent_session_id, status,
       current_revision, created_by)
    VALUES
      (v_proposal, v_project, branch_ids[v_parent_index], head_session_ids[v_parent_index],
       CASE WHEN i % 2 = 0 THEN 'awaiting_approval' ELSE 'draft' END, 1, 'agent');
    INSERT INTO goal_branch_proposal_revisions
      (id, proposal_id, revision, why_needed, contract, expected_contributions,
       exploration_plan, context_inheritance, tool_requirements, inferences,
       capability_policy, created_by)
    VALUES
      (gen_random_uuid(), v_proposal, 1, format('大型图中的待决定子目标 %s', i),
       jsonb_build_object(
         'desiredOutcome', format('Proposal %s：验证一个尚未批准的探索方向', lpad(i::text, 2, '0')),
         'hardConstraints', '[]'::jsonb, 'subjectivePreferences', '[]'::jsonb,
         'unknowns', jsonb_build_array('用户是否希望继续'), 'nonGoals', '[]'::jsonb,
         'validationPlan', jsonb_build_array('用户审查提案'),
         'judgmentTriggers', jsonb_build_array('批准前'),
         'stopConditions', jsonb_build_array('批准或取消'),
         'expectedContributions', '[]'::jsonb,
         'exploration', jsonb_build_object('mode','delivery','budgets','[]'::jsonb,'candidateOutputs','[]'::jsonb,'uncertaintyReduction','[]'::jsonb)),
       '[]'::jsonb, '[]'::jsonb, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
       '{}'::jsonb, 'agent');
  END LOOP;
END
$seed$;
COMMIT;
SQL

large_counts="$(docker exec "$large_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT count(*) FROM goal_branches WHERE project_id='$large_project_id'),
          (SELECT count(*) FROM goal_sessions WHERE project_id='$large_project_id'),
          (SELECT count(*) FROM goal_branch_proposals WHERE project_id='$large_project_id' AND status IN ('draft','awaiting_approval')),
          (SELECT count(*) FROM goal_review_gates WHERE project_id='$large_project_id'),
          (SELECT count(*) FROM goal_action_runs WHERE project_id='$large_project_id');")"
[[ "$large_counts" == '100|300|20|20|14' ]]

large_project_url="$large_base/projects/$large_project_id?tab=goals"
curl -fsS -o /dev/null "$large_project_url"
for large_measurement in 1 2 3; do
  curl -fsS -o "$large_tmp/page-$large_measurement.html" \
    -w '%{time_starttransfer}\n' "$large_project_url"
done > "$large_tmp/ttfb.txt"
large_ttfb_report="$(python3 -c 'import json,statistics,sys
values=[float(line) for line in open(sys.argv[1]) if line.strip()]
median=statistics.median(values)
assert median <= 1.5, {"valuesSeconds":values,"medianSeconds":median,"budgetSeconds":1.5}
print(json.dumps({"valuesSeconds":values,"medianSeconds":median,"budgetSeconds":1.5}, separators=(",",":")))' "$large_tmp/ttfb.txt")"

docker run --rm --init --ipc=host --network "$large_network" \
  -e BASE_URL=http://large-app:3000 -e PROJECT_ID="$large_project_id" \
  -e SCREENSHOT_DIR=/screenshots \
  --mount "type=bind,src=$large_repo_root,dst=/work" \
  --mount "type=bind,src=$large_screenshot_dir,dst=/screenshots" \
  --mount type=volume,src=fudian_playwright_npm_cache,dst=/root/.npm \
  "$large_browser_image" sh -c '
    npm install --prefix /tmp/pw --no-audit --no-fund @playwright/test@1.62.0 >/dev/null &&
    cp /work/tests/browser/workbench-large.spec.js /tmp/pw/workbench-large.spec.js &&
    cd /tmp/pw &&
    ./node_modules/.bin/playwright test workbench-large.spec.js --reporter=line --workers=1
  '

test -s "$large_screenshot_dir/goal-workbench-large-desktop.png"
test -s "$large_screenshot_dir/goal-workbench-large-mobile.png"
echo "large workbench passed: counts=$large_counts ttfb=$large_ttfb_report"
