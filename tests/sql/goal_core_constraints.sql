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

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_contract_versions
      (id, project_id, goal_branch_id, version, desired_outcome,
       validation_plan, stop_conditions, exploration_policy, supersedes_id, created_by)
    VALUES (
      '00000000-0000-0000-0000-000000000055',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      2, '非法探索契约', '["运行测试"]', '["测试通过"]',
      '{"mode":"unknown","budgets":[],"candidateOutputs":[],"uncertaintyReduction":[]}',
      '00000000-0000-0000-0000-000000000021', 'agent'
    );
    RAISE EXCEPTION 'invalid exploration policy reached the database';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

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

INSERT INTO goal_evidence
  (id, project_id, goal_branch_id, session_id, kind, stance, claim,
   observation, verification_status, content_hash, captured_by)
VALUES (
  '00000000-0000-0000-0000-000000000050',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  'test', 'supports', '单写者约束存在', '第二个 running Session 被拒绝',
  'verified', 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'agent'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_evidence SET observation = '被篡改'
    WHERE id = '00000000-0000-0000-0000-000000000050';
    RAISE EXCEPTION 'immutable Evidence accepted an update';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

INSERT INTO projects (id, title, intent, state)
VALUES (
  '00000000-0000-0000-0000-000000000002',
  '另一项目', '验证跨聚合引用被拒绝', 'active'
);

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_evidence
      (id, project_id, goal_branch_id, session_id, kind, stance, claim,
       observation, verification_status, content_hash, captured_by)
    VALUES (
      '00000000-0000-0000-0000-000000000051',
      '00000000-0000-0000-0000-000000000002',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000022',
      'test', 'supports', '非法跨项目', '不应保存', 'verified',
      'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      'agent'
    );
    RAISE EXCEPTION 'cross-project Evidence was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_contract_versions
  (id, project_id, goal_branch_id, version, desired_outcome,
   validation_plan, stop_conditions, supersedes_id, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000052',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  2, '完成更严格的约束测试', '["运行迁移约束"]', '["测试通过"]',
  '00000000-0000-0000-0000-000000000021', 'agent'
);

INSERT INTO goal_contract_revision_requests
  (id, project_id, goal_branch_id, based_on_contract_version_id,
   proposed_contract_version_id, proposed_by_session_id, reason,
   change_summary, created_by)
VALUES (
  '00000000-0000-0000-0000-000000000053',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000021',
  '00000000-0000-0000-0000-000000000052',
  '00000000-0000-0000-0000-000000000022',
  '验证契约差异不可改写',
  '[{"field":"desiredOutcome","before":"完成约束测试","after":"完成更严格的约束测试"}]',
  'agent'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_contract_revision_requests SET reason = '被篡改'
    WHERE id = '00000000-0000-0000-0000-000000000053';
    RAISE EXCEPTION 'contract revision identity accepted an update';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    DELETE FROM goal_contract_revision_requests
    WHERE id = '00000000-0000-0000-0000-000000000053';
    RAISE EXCEPTION 'contract revision request accepted deletion';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    UPDATE goal_contract_revision_requests
    SET status = 'accepted', updated_at = now(), decided_at = now()
    WHERE id = '00000000-0000-0000-0000-000000000053';
    SET CONSTRAINTS goal_contract_revision_decision_required IMMEDIATE;
    RAISE EXCEPTION 'terminal contract revision without a human decision was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

BEGIN;
UPDATE goal_contract_revision_requests
SET status = 'rejected', updated_at = now(), decided_at = now()
WHERE id = '00000000-0000-0000-0000-000000000053';
INSERT INTO goal_contract_revision_decisions
  (id, project_id, revision_request_id, actor_role, decision, rationale)
VALUES (
  '00000000-0000-0000-0000-000000000054',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000053',
  'human', 'reject', '仅验证数据库状态机'
);
COMMIT;

INSERT INTO goal_review_gates
  (id, project_id, goal_branch_id, session_id, contract_version_id,
   candidate_snapshot, candidate_hash)
VALUES (
  '00000000-0000-0000-0000-000000000060',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000021',
  '{"contributionIds":[],"evidenceIds":[]}',
  'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_review_gates
    SET candidate_snapshot = '{"contributionIds":["tampered"],"evidenceIds":[]}'
    WHERE id = '00000000-0000-0000-0000-000000000060';
    RAISE EXCEPTION 'frozen ReviewGate accepted candidate mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    UPDATE goal_review_gates SET status = 'accepted'
    WHERE id = '00000000-0000-0000-0000-000000000060';
    RAISE EXCEPTION 'ReviewGate skipped independent review';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    UPDATE goal_branches
    SET status = 'archived', archived_from_status = 'completed'
    WHERE id = '00000000-0000-0000-0000-000000000020';
    RAISE EXCEPTION 'active GoalBranch was archived with a forged conclusion';
  EXCEPTION WHEN check_violation THEN
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
