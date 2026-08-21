BEGIN;

INSERT INTO projects (id, title, intent, state)
VALUES (
  '00000000-0000-0000-0000-000000000001',
  '目标枝干约束测试',
  '验证批准、单写者与不可变审计',
  'active'
);

INSERT INTO goal_branch_proposals
  (id, project_id, status, current_revision, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000010',
  '00000000-0000-0000-0000-000000000001',
  'awaiting_approval', 1, 'agent'
);

INSERT INTO goal_branch_proposal_revisions
  (id, proposal_id, revision, why_needed, contract, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000011',
  '00000000-0000-0000-0000-000000000010',
  1, '验证核心枝干', '{"desiredOutcome":"完成约束测试"}', 'agent'
);

INSERT INTO goal_branches
  (id, project_id, creating_proposal_id, name,
   current_contract_version_id, head_session_id)
VALUES (
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000010',
  '根目标',
  '00000000-0000-0000-0000-000000000021',
  '00000000-0000-0000-0000-000000000022'
);

INSERT INTO goal_contract_versions
  (id, project_id, goal_branch_id, version, desired_outcome,
   validation_plan, stop_conditions, source_proposal_id, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000021',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  1, '完成约束测试', '["运行测试"]', '["测试通过"]',
  '00000000-0000-0000-0000-000000000010', 'human'
);

INSERT INTO goal_sessions
  (id, project_id, goal_branch_id, session_number, status,
   assignment, contract_version_id)
VALUES (
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  1, 'running', '验证第一轮',
  '00000000-0000-0000-0000-000000000021'
);

UPDATE goal_branch_proposals
SET status = 'approved',
    approved_revision = 1,
    approved_goal_branch_id = '00000000-0000-0000-0000-000000000020',
    decided_at = now()
WHERE id = '00000000-0000-0000-0000-000000000010';

COMMIT;

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_sessions
      (id, project_id, goal_branch_id, session_number, status,
       assignment, contract_version_id)
    VALUES (
      '00000000-0000-0000-0000-000000000023',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      2, 'running', '非法并发写者',
      '00000000-0000-0000-0000-000000000021'
    );
    RAISE EXCEPTION 'single-writer constraint did not reject a second writer';
  EXCEPTION WHEN unique_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_events
  (id, project_id, aggregate_type, aggregate_id, event_type,
   actor_type, client_request_id, payload)
VALUES (
  '00000000-0000-0000-0000-000000000030',
  '00000000-0000-0000-0000-000000000001',
  'session', '00000000-0000-0000-0000-000000000022',
  'session.started', 'system',
  '00000000-0000-0000-0000-000000000031', '{}'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_events
    SET event_type = 'tampered'
    WHERE id = '00000000-0000-0000-0000-000000000030';
    RAISE EXCEPTION 'immutable trigger did not reject an event update';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

BEGIN;
INSERT INTO goal_branch_proposals
  (id, project_id, status, current_revision, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000040',
  '00000000-0000-0000-0000-000000000001',
  'draft', 1, 'agent'
);
INSERT INTO goal_branch_proposal_revisions
  (id, proposal_id, revision, why_needed, contract, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000041',
  '00000000-0000-0000-0000-000000000040',
  1, '未批准测试', '{"desiredOutcome":"不能绕过批准"}', 'agent'
);
COMMIT;

DO $$
BEGIN
  BEGIN
    UPDATE goal_branches
    SET creating_proposal_id = '00000000-0000-0000-0000-000000000040'
    WHERE id = '00000000-0000-0000-0000-000000000020';
    SET CONSTRAINTS goal_branches_approved_origin IMMEDIATE;
    RAISE EXCEPTION 'an unapproved Proposal was accepted as branch origin';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  IF (SELECT count(*) FROM goal_branches) <> 1
     OR (SELECT count(*) FROM goal_sessions WHERE status = 'running') <> 1
     OR (SELECT count(*) FROM goal_events) <> 1 THEN
    RAISE EXCEPTION 'goal core constraint fixture ended in an unexpected state';
  END IF;
END;
$$;
