BEGIN;
SET CONSTRAINTS ALL DEFERRED;

INSERT INTO projects (id, title, intent, state)
VALUES (
  '20000000-0000-0000-0000-000000000001',
  '调度约束测试', '验证 ActionRun、fencing 与通知不可变性', 'active'
);

INSERT INTO goal_branch_proposals
  (id, project_id, status, current_revision, approved_revision,
   approved_goal_branch_id, created_by, decided_at)
VALUES (
  '20000000-0000-0000-0000-000000000010',
  '20000000-0000-0000-0000-000000000001',
  'approved', 1, 1, '20000000-0000-0000-0000-000000000020', 'agent', now()
);

INSERT INTO goal_branch_proposal_revisions
  (id, proposal_id, revision, why_needed, contract, created_by)
VALUES (
  '20000000-0000-0000-0000-000000000011',
  '20000000-0000-0000-0000-000000000010',
  1, '验证持久调度', '{"desiredOutcome":"调度约束成立"}', 'agent'
);

INSERT INTO goal_branches
  (id, project_id, creating_proposal_id, name, status,
   current_contract_version_id, head_session_id)
VALUES (
  '20000000-0000-0000-0000-000000000020',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000010',
  '调度目标', 'active',
  '20000000-0000-0000-0000-000000000021',
  '20000000-0000-0000-0000-000000000022'
);

INSERT INTO goal_contract_versions
  (id, project_id, goal_branch_id, version, desired_outcome,
   validation_plan, stop_conditions, source_proposal_id, created_by)
VALUES (
  '20000000-0000-0000-0000-000000000021',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000020',
  1, '调度约束成立', '["运行约束"]', '["全部通过"]',
  '20000000-0000-0000-0000-000000000010', 'human'
);

INSERT INTO goal_sessions
  (id, project_id, goal_branch_id, session_number, status, assignment,
   contract_version_id)
VALUES (
  '20000000-0000-0000-0000-000000000022',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000020',
  1, 'running', '调度约束测试',
  '20000000-0000-0000-0000-000000000021'
);

INSERT INTO scheduler_workers
  (id, client_request_id, display_name, token_digest, capabilities)
VALUES (
  '20000000-0000-0000-0000-000000000030',
  '20000000-0000-0000-0000-000000000031',
  'constraint worker', 'sha256:' || repeat('1', 64),
  '["agent.execute","scheduler.reconcile"]'
);

INSERT INTO goal_action_runs
  (id, project_id, goal_branch_id, session_id, client_request_id, request_hash,
   kind, capability, subject_kind, payload, retry_safety, max_attempts)
VALUES (
  '20000000-0000-0000-0000-000000000040',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000020',
  '20000000-0000-0000-0000-000000000022',
  '20000000-0000-0000-0000-000000000041',
  'sha256:' || repeat('2', 64),
  'agent_step', 'agent.execute', 'none', '{}', 'safe', 3
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_action_runs SET payload = '{"changed":true}'
    WHERE id = '20000000-0000-0000-0000-000000000040';
    RAISE EXCEPTION 'ActionRun identity mutation was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE goal_action_runs SET status = 'running'
    WHERE id = '20000000-0000-0000-0000-000000000040';
    RAISE EXCEPTION 'ActionRun claim without counters was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

UPDATE goal_action_runs
SET status = 'running', attempt_count = 1, fencing_counter = 1,
    started_at = now(), updated_at = now()
WHERE id = '20000000-0000-0000-0000-000000000040';

INSERT INTO action_run_leases
  (id, action_run_id, worker_id, claim_request_id, attempt_number, fencing_token,
   renewal_token_digest, soft_expires_at, hard_expires_at)
VALUES (
  '20000000-0000-0000-0000-000000000050',
  '20000000-0000-0000-0000-000000000040',
  '20000000-0000-0000-0000-000000000030',
  '20000000-0000-0000-0000-000000000051',
  1, 1, 'sha256:' || repeat('3', 64),
  now() + interval '10 seconds', now() + interval '5 minutes'
);
SET CONSTRAINTS ALL IMMEDIATE;

DO $$
BEGIN
  BEGIN
    INSERT INTO action_run_leases
      (id, action_run_id, worker_id, claim_request_id, attempt_number, fencing_token,
       renewal_token_digest, soft_expires_at, hard_expires_at)
    VALUES (
      '20000000-0000-0000-0000-000000000052',
      '20000000-0000-0000-0000-000000000040',
      '20000000-0000-0000-0000-000000000030',
      '20000000-0000-0000-0000-000000000053',
      2, 2, 'sha256:' || repeat('4', 64),
      now() + interval '10 seconds', now() + interval '5 minutes'
    );
    RAISE EXCEPTION 'second active ActionLease was accepted';
  EXCEPTION WHEN unique_violation THEN
    NULL;
  END;
  BEGIN
    UPDATE action_run_leases SET soft_expires_at = acquired_at - interval '1 second'
    WHERE id = '20000000-0000-0000-0000-000000000050';
    RAISE EXCEPTION 'ActionLease expiry moved backwards';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

UPDATE action_run_leases
SET status = 'succeeded', outcome = '{"ok":true}', completed_at = now()
WHERE id = '20000000-0000-0000-0000-000000000050';
UPDATE goal_action_runs
SET status = 'succeeded', result = '{"ok":true}', updated_at = now(), completed_at = now()
WHERE id = '20000000-0000-0000-0000-000000000040';

DO $$
BEGIN
  BEGIN
    UPDATE action_run_leases SET outcome = '{"forged":true}'
    WHERE id = '20000000-0000-0000-0000-000000000050';
    RAISE EXCEPTION 'terminal ActionLease accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE goal_action_runs SET last_error_summary = 'forged'
    WHERE id = '20000000-0000-0000-0000-000000000040';
    RAISE EXCEPTION 'terminal ActionRun accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_notifications
  (id, project_id, goal_branch_id, session_id, action_run_id, dedupe_key,
   kind, severity, title, summary)
VALUES (
  '20000000-0000-0000-0000-000000000060',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000020',
  '20000000-0000-0000-0000-000000000022',
  '20000000-0000-0000-0000-000000000040',
  'constraint-notification', 'action_failed', 'warning', '等待处理', '约束测试'
);

INSERT INTO notification_outbox
  (id, notification_id, adapter, dedupe_key, payload, payload_digest,
   status, last_error, completed_at)
VALUES (
  '20000000-0000-0000-0000-000000000061',
  '20000000-0000-0000-0000-000000000060',
  'none', 'none:constraint', '{}', 'sha256:' || repeat('5', 64),
  'suppressed', 'adapter not configured', now()
);

DO $$
BEGIN
  BEGIN
    DELETE FROM goal_notifications
    WHERE id = '20000000-0000-0000-0000-000000000060';
    RAISE EXCEPTION 'Notification deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE notification_outbox SET last_error = 'changed'
    WHERE id = '20000000-0000-0000-0000-000000000061';
    RAISE EXCEPTION 'terminal Notification delivery accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    DELETE FROM scheduler_workers
    WHERE id = '20000000-0000-0000-0000-000000000030';
    RAISE EXCEPTION 'SchedulerWorker deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

INSERT INTO tool_leases
  (id, project_id, goal_branch_id, session_id, plugin_id, plugin_version,
   plugin_digest, tool_name, environment_fingerprint, base_workspace_snapshot,
   status, renewal_token_digest, resource_policy, soft_expires_at, hard_expires_at)
VALUES (
  '20000000-0000-0000-0000-000000000070',
  '20000000-0000-0000-0000-000000000001',
  '20000000-0000-0000-0000-000000000020',
  '20000000-0000-0000-0000-000000000022',
  'fudian.tools.constraint', '1.0.0', 'sha256:' || repeat('6', 64),
  'serve', 'sha256:' || repeat('7', 64), 'sha256:' || repeat('8', 64),
  'requested', 'sha256:' || repeat('9', 64), '{}',
  now() + interval '10 seconds', now() + interval '5 minutes'
);
UPDATE tool_leases
SET status = 'active', endpoint_refs = '["http://fixture:4173"]',
    last_heartbeat_at = now(), soft_expires_at = soft_expires_at + interval '1 second',
    cleanup_status = 'pending'
WHERE id = '20000000-0000-0000-0000-000000000070';
UPDATE tool_leases
SET status = 'released', cleanup_status = 'succeeded', completed_at = now()
WHERE id = '20000000-0000-0000-0000-000000000070';

DO $$
BEGIN
  BEGIN
    DELETE FROM tool_leases
    WHERE id = '20000000-0000-0000-0000-000000000070';
    RAISE EXCEPTION 'ToolLease deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

ROLLBACK;
