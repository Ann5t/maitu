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

INSERT INTO goal_context_entries
  (id, project_id, origin_goal_branch_id, source_kind, source_record_id,
   title, content_hash, importance)
VALUES (
  '00000000-0000-0000-0000-000000000070',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  'contract', '00000000-0000-0000-0000-000000000021',
  '不可折叠契约',
  'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
  'essential'
);

INSERT INTO goal_context_snapshots
  (id, project_id, goal_branch_id, session_id, version, contract_version_id,
   required_context, required_context_hash, budget_policy, catalog_hash)
VALUES (
  '00000000-0000-0000-0000-000000000071',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022', 1,
  '00000000-0000-0000-0000-000000000021',
  '{"nonFoldable":true,"contract":{},"ancestorContracts":[],"permissions":{},"creationState":{},"unresolvedAttention":[]}',
  'sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
  '{"requiredContextUnabridged":true,"maxCatalogItems":12}',
  'sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'
);

INSERT INTO goal_context_snapshot_entries
  (snapshot_id, entry_id, inheritance_kind, rank, inclusion_reason)
VALUES (
  '00000000-0000-0000-0000-000000000071',
  '00000000-0000-0000-0000-000000000070',
  'required', 0, '契约不可折叠'
);

UPDATE goal_sessions
SET context_snapshot_id = '00000000-0000-0000-0000-000000000071'
WHERE id = '00000000-0000-0000-0000-000000000022';

INSERT INTO goal_context_derivations
  (id, project_id, entry_id, kind, generation, generator, source_hash,
   payload, content_hash)
VALUES (
  '00000000-0000-0000-0000-000000000072',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000070',
  'summary', 1, 'constraint-test',
  'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
  '{"text":"不可折叠契约"}',
  'sha256:1111111111111111111111111111111111111111111111111111111111111111'
);

INSERT INTO goal_context_reads
  (id, project_id, goal_branch_id, session_id, snapshot_id, entry_id,
   client_request_id, request_hash, disclosure_level, purpose, source_hash,
   result_hash, result_chars, actor_type)
VALUES (
  '00000000-0000-0000-0000-000000000074',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000071',
  '00000000-0000-0000-0000-000000000070',
  '00000000-0000-0000-0000-000000000075',
  'sha256:2222222222222222222222222222222222222222222222222222222222222222',
  'summary', '验证读取审计',
  'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
  'sha256:3333333333333333333333333333333333333333333333333333333333333333',
  7, 'agent'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_context_entries SET title = '篡改'
    WHERE id = '00000000-0000-0000-0000-000000000070';
    RAISE EXCEPTION 'immutable ContextEntry accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    UPDATE goal_context_snapshots SET catalog_hash =
      'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    WHERE id = '00000000-0000-0000-0000-000000000071';
    RAISE EXCEPTION 'immutable ContextSnapshot accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_context_entries
      (id, project_id, origin_goal_branch_id, source_kind, source_record_id,
       title, content_hash)
    VALUES (
      '00000000-0000-0000-0000-000000000076',
      '00000000-0000-0000-0000-000000000002',
      '00000000-0000-0000-0000-000000000020',
      'contract', '00000000-0000-0000-0000-000000000021', '跨项目来源',
      'sha256:4444444444444444444444444444444444444444444444444444444444444444'
    );
    RAISE EXCEPTION 'cross-project ContextEntry was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_context_derivations
      (id, project_id, entry_id, kind, generation, generator, source_hash,
       payload, content_hash)
    VALUES (
      '00000000-0000-0000-0000-000000000077',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000070',
      'summary', 2, 'forged',
      'sha256:5555555555555555555555555555555555555555555555555555555555555555',
      '{"text":"伪造"}',
      'sha256:6666666666666666666666666666666666666666666666666666666666666666'
    );
    RAISE EXCEPTION 'derivation with wrong source hash was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_context_entries
  (id, project_id, origin_goal_branch_id, origin_session_id, source_kind,
   source_record_id, title, content_hash)
VALUES (
  '00000000-0000-0000-0000-000000000078',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  'evidence', '00000000-0000-0000-0000-000000000050', '目录之外的 Evidence',
  'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
);

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_context_reads
      (id, project_id, goal_branch_id, session_id, snapshot_id, entry_id,
       client_request_id, request_hash, disclosure_level, purpose, source_hash,
       result_hash, result_chars, actor_type)
    VALUES (
      '00000000-0000-0000-0000-000000000079',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000022',
      '00000000-0000-0000-0000-000000000071',
      '00000000-0000-0000-0000-000000000078',
      '00000000-0000-0000-0000-000000000080',
      'sha256:7777777777777777777777777777777777777777777777777777777777777777',
      'full', '非法读取目录之外的来源',
      'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      'sha256:8888888888888888888888888888888888888888888888888888888888888888',
      1, 'agent'
    );
    RAISE EXCEPTION 'ContextRead outside snapshot membership was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_sessions
  (id, project_id, goal_branch_id, session_number, status,
   assignment, contract_version_id)
VALUES (
  '00000000-0000-0000-0000-000000000073',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  2, 'stopped', '验证跨 Session 指针',
  '00000000-0000-0000-0000-000000000021'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_sessions
    SET context_snapshot_id = '00000000-0000-0000-0000-000000000071'
    WHERE id = '00000000-0000-0000-0000-000000000073';
    RAISE EXCEPTION 'cross-session current context pointer was accepted';
  EXCEPTION WHEN check_violation THEN
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

INSERT INTO project_git_repositories
  (id, project_id, storage_key, default_branch, default_head_commit, object_format)
VALUES (
  '00000000-0000-0000-0000-000000000090',
  '00000000-0000-0000-0000-000000000001',
  'projects/00000000-0000-0000-0000-000000000001.git',
  'main', '1111111111111111111111111111111111111111', 'sha1'
);

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_workspace_policies
      (id, project_id, goal_branch_id, source_proposal_id,
       source_proposal_revision, policy, policy_hash)
    VALUES (
      '00000000-0000-0000-0000-000000000097',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000040', 1, '{}',
      'sha256:4444444444444444444444444444444444444444444444444444444444444444'
    );
    RAISE EXCEPTION 'workspace policy accepted the wrong origin Proposal';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_workspace_policies
  (id, project_id, goal_branch_id, source_proposal_id,
   source_proposal_revision, policy, policy_hash)
VALUES (
  '00000000-0000-0000-0000-000000000091',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000010', 1, '{}',
  'sha256:4444444444444444444444444444444444444444444444444444444444444444'
);

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_workspaces
      (id, project_id, goal_branch_id, repository_id, git_branch_name,
       worktree_key, status)
    VALUES (
      '00000000-0000-0000-0000-000000000098',
      '00000000-0000-0000-0000-000000000002',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000090',
      'goal/00000000-0000-0000-0000-000000000020',
      'projects/00000000-0000-0000-0000-000000000002/goals/00000000-0000-0000-0000-000000000020',
      'provisioning'
    );
    RAISE EXCEPTION 'cross-project GoalWorkspace was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_workspaces
  (id, project_id, goal_branch_id, repository_id, git_branch_name,
   worktree_key, base_commit, head_commit, tree_id, workspace_snapshot,
   dirty, status)
VALUES (
  '00000000-0000-0000-0000-000000000092',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000090',
  'goal/00000000-0000-0000-0000-000000000020',
  'projects/00000000-0000-0000-0000-000000000001/goals/00000000-0000-0000-0000-000000000020',
  '1111111111111111111111111111111111111111',
  '1111111111111111111111111111111111111111',
  '2222222222222222222222222222222222222222',
  'sha256:3333333333333333333333333333333333333333333333333333333333333333',
  false, 'ready'
);

INSERT INTO workspace_operations
  (id, project_id, goal_branch_id, workspace_id, operation_kind, status,
   request_hash, expected_head_commit, candidate_commit, completed_at)
VALUES (
  '00000000-0000-0000-0000-000000000093',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000092',
  'provision', 'applied',
  'sha256:5555555555555555555555555555555555555555555555555555555555555555',
  '1111111111111111111111111111111111111111',
  '1111111111111111111111111111111111111111', now()
);

DO $$
BEGIN
  BEGIN
    INSERT INTO workspace_operations
      (id, project_id, goal_branch_id, workspace_id, operation_kind, status,
       request_hash)
    VALUES (
      '00000000-0000-0000-0000-000000000099',
      '00000000-0000-0000-0000-000000000002',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000092',
      'reconcile', 'planned',
      'sha256:5555555555555555555555555555555555555555555555555555555555555555'
    );
    RAISE EXCEPTION 'cross-project WorkspaceOperation was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO workspace_snapshots
  (id, project_id, goal_branch_id, session_id, workspace_id, operation_id,
   head_commit, tree_id, dirty, snapshot_hash)
VALUES (
  '00000000-0000-0000-0000-000000000094',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000092',
  '00000000-0000-0000-0000-000000000093',
  '1111111111111111111111111111111111111111',
  '2222222222222222222222222222222222222222', false,
  'sha256:3333333333333333333333333333333333333333333333333333333333333333'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_workspace_policies SET policy = '{"deployment":true}'
    WHERE id = '00000000-0000-0000-0000-000000000091';
    RAISE EXCEPTION 'immutable WorkspacePolicy accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE workspace_snapshots SET dirty = true
    WHERE id = '00000000-0000-0000-0000-000000000094';
    RAISE EXCEPTION 'immutable WorkspaceSnapshot accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

INSERT INTO workspace_write_leases
  (id, project_id, goal_branch_id, session_id, workspace_id,
   client_request_id, request_hash, status, fencing_token,
   renewal_token_digest, base_commit, base_workspace_snapshot,
   allowed_writes, capabilities, resource_policy, output_key,
   soft_expires_at, hard_expires_at)
VALUES (
  '00000000-0000-0000-0000-000000000095',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000092',
  '00000000-0000-0000-0000-000000000195',
  'sha256:6666666666666666666666666666666666666666666666666666666666666666',
  'active', 1,
  'sha256:7777777777777777777777777777777777777777777777777777777777777777',
  '1111111111111111111111111111111111111111',
  'sha256:3333333333333333333333333333333333333333333333333333333333333333',
  '["docs/**"]', '{}', '{}',
  'jobs/00000000-0000-0000-0000-000000000095',
  now() + interval '5 minutes', now() + interval '10 minutes'
);

DO $$
BEGIN
  BEGIN
    INSERT INTO workspace_write_leases
      (id, project_id, goal_branch_id, session_id, workspace_id,
       client_request_id, request_hash, status, fencing_token,
       renewal_token_digest, base_commit, base_workspace_snapshot,
       allowed_writes, capabilities, resource_policy, output_key,
       soft_expires_at, hard_expires_at)
    VALUES (
      '00000000-0000-0000-0000-000000000101',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000022',
      '00000000-0000-0000-0000-000000000092',
      '00000000-0000-0000-0000-000000000201',
      'sha256:8888888888888888888888888888888888888888888888888888888888888888',
      'active', 2,
      'sha256:9999999999999999999999999999999999999999999999999999999999999999',
      '1111111111111111111111111111111111111111',
      'sha256:3333333333333333333333333333333333333333333333333333333333333333',
      '["docs/**"]', '{}', '{}',
      'jobs/00000000-0000-0000-0000-000000000101',
      now() + interval '5 minutes', now() + interval '10 minutes'
    );
    RAISE EXCEPTION 'second active Workspace Lease was accepted';
  EXCEPTION WHEN unique_violation THEN
    NULL;
  END;
  BEGIN
    UPDATE workspace_write_leases SET fencing_token = 9
    WHERE id = '00000000-0000-0000-0000-000000000095';
    RAISE EXCEPTION 'Workspace Lease identity accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE workspace_write_leases SET delete_paths = '["docs/old.md"]'
    WHERE id = '00000000-0000-0000-0000-000000000095';
    RAISE EXCEPTION 'Workspace Lease deletion identity accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

INSERT INTO runner_jobs
  (id, project_id, goal_branch_id, session_id, workspace_id, lease_id,
   client_request_id, request_hash, status, spec, spec_hash, runtime_digest)
VALUES (
  '00000000-0000-0000-0000-000000000096',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000092',
  '00000000-0000-0000-0000-000000000095',
  '00000000-0000-0000-0000-000000000196',
  'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'prepared', '{}',
  'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
);

INSERT INTO runner_job_files
  (runner_job_id, project_id, path, sha256, size_bytes, executable)
VALUES (
  '00000000-0000-0000-0000-000000000096',
  '00000000-0000-0000-0000-000000000001',
  'docs/readme..md',
  'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
  12, false
);

DO $$
BEGIN
  BEGIN
    INSERT INTO runner_job_files
      (runner_job_id, project_id, path, sha256, size_bytes, executable)
    VALUES (
      '00000000-0000-0000-0000-000000000096',
      '00000000-0000-0000-0000-000000000001', '.git/config',
      'sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
      1, false
    );
    RAISE EXCEPTION 'Runner output accepted reserved .git metadata path';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
  BEGIN
    INSERT INTO runner_job_files
      (runner_job_id, project_id, path, sha256, size_bytes, executable)
    VALUES (
      '00000000-0000-0000-0000-000000000096',
      '00000000-0000-0000-0000-000000000001', 'docs/README..md',
      'sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
      1, false
    );
    RAISE EXCEPTION 'Runner output accepted a case-folded path collision';
  EXCEPTION WHEN unique_violation THEN
    NULL;
  END;
  BEGIN
    UPDATE runner_job_files SET size_bytes = 99
    WHERE runner_job_id = '00000000-0000-0000-0000-000000000096';
    RAISE EXCEPTION 'immutable RunnerJobFile accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE runner_jobs SET runtime_digest =
      'sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'
    WHERE id = '00000000-0000-0000-0000-000000000096';
    RAISE EXCEPTION 'RunnerJob request identity accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

UPDATE runner_jobs
SET status = 'failed', result = '{"reason":"constraint test"}', completed_at = now()
WHERE id = '00000000-0000-0000-0000-000000000096';

DO $$
BEGIN
  BEGIN
    UPDATE runner_jobs SET result = '{"reason":"tampered"}'
    WHERE id = '00000000-0000-0000-0000-000000000096';
    RAISE EXCEPTION 'terminal RunnerJob accepted mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

UPDATE workspace_write_leases
SET status = 'released', completed_at = now()
WHERE id = '00000000-0000-0000-0000-000000000095';

DO $$
BEGIN
  BEGIN
    UPDATE workspace_write_leases SET completed_at = now() + interval '1 minute'
    WHERE id = '00000000-0000-0000-0000-000000000095';
    RAISE EXCEPTION 'terminal Workspace Lease accepted mutation';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    INSERT INTO goal_review_gates
      (id, project_id, goal_branch_id, session_id, contract_version_id,
       candidate_snapshot, candidate_hash, workspace_id)
    VALUES (
      '00000000-0000-0000-0000-000000000160',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000073',
      '00000000-0000-0000-0000-000000000021',
      '{"contributionIds":[],"evidenceIds":[]}',
      'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      '00000000-0000-0000-0000-000000000092'
    );
    RAISE EXCEPTION 'ReviewGate accepted a partial physical binding';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
  BEGIN
    INSERT INTO goal_review_decisions
      (id, project_id, review_gate_id, actor_role, actor_identity, decision,
       rationale, candidate_digest)
    VALUES (
      '00000000-0000-0000-0000-000000000161',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000060',
      'review_ai', 'forged-worker', 'recommend_accept', '缺少租约绑定',
      'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
    );
    RAISE EXCEPTION 'ReviewDecision accepted a partial Worker binding';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
  BEGIN
    INSERT INTO goal_integrations
      (id, project_id, source_goal_branch_id, target_goal_branch_id,
       review_gate_id, kind, summary, git_integration_status, source_workspace_id)
    VALUES (
      '00000000-0000-0000-0000-000000000162',
      '00000000-0000-0000-0000-000000000001',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000020',
      '00000000-0000-0000-0000-000000000060',
      'full', '缺少父基线与摘要', 'pending',
      '00000000-0000-0000-0000-000000000092'
    );
    RAISE EXCEPTION 'GoalIntegration accepted a partial physical binding';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO goal_integrations
  (id, project_id, source_goal_branch_id, target_goal_branch_id,
   review_gate_id, kind, summary, git_integration_status)
VALUES (
  '00000000-0000-0000-0000-000000000163',
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000020', NULL,
  '00000000-0000-0000-0000-000000000060',
  'full', '兼容旧逻辑回执', 'not_attempted'
);

DO $$
BEGIN
  BEGIN
    UPDATE goal_integrations SET summary = '篡改冻结身份'
    WHERE id = '00000000-0000-0000-0000-000000000163';
    RAISE EXCEPTION 'GoalIntegration accepted frozen identity mutation';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    UPDATE goal_integrations SET git_integration_status = 'applied'
    WHERE id = '00000000-0000-0000-0000-000000000163';
    RAISE EXCEPTION 'GoalIntegration skipped the physical state machine';
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
