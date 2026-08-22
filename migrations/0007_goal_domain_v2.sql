ALTER TABLE goal_branches
  ADD COLUMN IF NOT EXISTS archived_from_status text CHECK (
    archived_from_status IS NULL OR archived_from_status IN (
      'integrated', 'completed', 'stopped'
    )
  );

ALTER TABLE goal_contract_versions
  ADD COLUMN IF NOT EXISTS exploration_policy jsonb NOT NULL DEFAULT
    '{"mode":"delivery","budgets":[],"candidateOutputs":[],"uncertaintyReduction":[]}'::jsonb
  CHECK (jsonb_typeof(exploration_policy) = 'object');

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_contract_versions_exploration_policy_shape'
      AND conrelid = 'goal_contract_versions'::regclass
  ) THEN
    ALTER TABLE goal_contract_versions
      ADD CONSTRAINT goal_contract_versions_exploration_policy_shape CHECK (
        exploration_policy ?& ARRAY[
          'mode', 'budgets', 'candidateOutputs', 'uncertaintyReduction'
        ]
        AND exploration_policy->>'mode' IN ('delivery', 'exploration', 'hybrid')
        AND jsonb_typeof(exploration_policy->'budgets') = 'array'
        AND jsonb_typeof(exploration_policy->'candidateOutputs') = 'array'
        AND jsonb_typeof(exploration_policy->'uncertaintyReduction') = 'array'
        AND CASE WHEN exploration_policy->>'mode' IN ('exploration', 'hybrid') THEN
          jsonb_array_length(exploration_policy->'budgets') > 0
          AND jsonb_array_length(exploration_policy->'candidateOutputs') > 0
          AND jsonb_array_length(exploration_policy->'uncertaintyReduction') > 0
        ELSE true END
      );
  END IF;
END $$;

ALTER TABLE goal_attention_items
  DROP CONSTRAINT IF EXISTS goal_attention_items_kind_check;
ALTER TABLE goal_attention_items
  ADD CONSTRAINT goal_attention_items_kind_check CHECK (kind IN (
    'branch_review', 'dependency', 'judgment', 'merge_review', 'exception',
    'manual_pause', 'continuation_required', 'child_result_ready', 'contract_review'
  ));

ALTER TABLE goal_events
  DROP CONSTRAINT IF EXISTS goal_events_aggregate_type_check;
ALTER TABLE goal_events
  ADD CONSTRAINT goal_events_aggregate_type_check CHECK (aggregate_type IN (
    'proposal', 'goal_branch', 'session', 'contract', 'contract_revision',
    'evidence', 'review_gate', 'integration', 'attention', 'tool',
    'input_artifact', 'project'
  ));

CREATE TABLE IF NOT EXISTS goal_contract_revision_requests (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  based_on_contract_version_id uuid NOT NULL
    REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  proposed_contract_version_id uuid NOT NULL UNIQUE
    REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  proposed_by_session_id uuid REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  status text NOT NULL DEFAULT 'awaiting_approval' CHECK (status IN (
    'awaiting_approval', 'accepted', 'rejected'
  )),
  reason text NOT NULL,
  change_summary jsonb NOT NULL CHECK (jsonb_typeof(change_summary) = 'array'),
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  CHECK (based_on_contract_version_id <> proposed_contract_version_id),
  CHECK (
    (status = 'awaiting_approval' AND decided_at IS NULL)
    OR (status IN ('accepted', 'rejected') AND decided_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_contract_one_pending_revision_idx
  ON goal_contract_revision_requests (goal_branch_id)
  WHERE status = 'awaiting_approval';

CREATE TABLE IF NOT EXISTS goal_contract_revision_decisions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  revision_request_id uuid NOT NULL UNIQUE
    REFERENCES goal_contract_revision_requests(id) ON DELETE RESTRICT,
  actor_role text NOT NULL CHECK (actor_role = 'human'),
  decision text NOT NULL CHECK (decision IN ('accept', 'reject')),
  rationale text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS goal_contract_provenance (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  contract_version_id uuid NOT NULL
    REFERENCES goal_contract_versions(id) ON DELETE CASCADE,
  field_path text NOT NULL,
  source_kind text NOT NULL CHECK (source_kind IN (
    'human_input', 'agent_inference', 'external_source', 'inherited_contract',
    'artifact', 'evidence'
  )),
  source_ref text,
  note text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (contract_version_id, field_path, source_kind, source_ref)
);

CREATE TABLE IF NOT EXISTS goal_evidence (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  kind text NOT NULL CHECK (kind IN (
    'test', 'browser', 'observation', 'external_source', 'artifact',
    'tool_result', 'research'
  )),
  stance text NOT NULL CHECK (stance IN ('supports', 'refutes', 'blocks', 'context')),
  claim text NOT NULL,
  observation text NOT NULL,
  source_uri text,
  artifact_id uuid REFERENCES artifacts(id) ON DELETE SET NULL,
  tool_call_id uuid REFERENCES tool_calls(id) ON DELETE SET NULL,
  verification_status text NOT NULL CHECK (verification_status IN (
    'unverified', 'verified', 'failed'
  )),
  content_hash text NOT NULL CHECK (content_hash ~ '^sha256:[0-9a-f]{64}$'),
  captured_by text NOT NULL CHECK (captured_by IN ('human', 'agent', 'review_ai', 'system')),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS goal_contribution_evidence (
  contribution_id uuid NOT NULL REFERENCES goal_contributions(id) ON DELETE CASCADE,
  evidence_id uuid NOT NULL REFERENCES goal_evidence(id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (contribution_id, evidence_id)
);

CREATE TABLE IF NOT EXISTS goal_review_gate_evidence (
  review_gate_id uuid NOT NULL REFERENCES goal_review_gates(id) ON DELETE CASCADE,
  evidence_id uuid NOT NULL REFERENCES goal_evidence(id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (review_gate_id, evidence_id)
);

CREATE INDEX IF NOT EXISTS goal_contract_revision_project_idx
  ON goal_contract_revision_requests (project_id, created_at, id);
CREATE INDEX IF NOT EXISTS goal_contract_provenance_contract_idx
  ON goal_contract_provenance (contract_version_id, field_path, id);
CREATE INDEX IF NOT EXISTS goal_evidence_branch_idx
  ON goal_evidence (goal_branch_id, created_at, id);

CREATE OR REPLACE FUNCTION goal_validate_proposal_parent_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  parent_branch_project uuid;
  parent_session_project uuid;
  parent_session_branch uuid;
BEGIN
  IF NEW.parent_goal_branch_id IS NULL THEN
    RETURN NEW;
  END IF;
  SELECT project_id INTO parent_branch_project
  FROM goal_branches WHERE id = NEW.parent_goal_branch_id;
  SELECT project_id, goal_branch_id INTO parent_session_project, parent_session_branch
  FROM goal_sessions WHERE id = NEW.parent_session_id;
  IF parent_branch_project IS DISTINCT FROM NEW.project_id
     OR parent_session_project IS DISTINCT FROM NEW.project_id
     OR parent_session_branch IS DISTINCT FROM NEW.parent_goal_branch_id THEN
    RAISE EXCEPTION 'BranchProposal parent references cross aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_branch_proposals_parent_scope
    AFTER INSERT OR UPDATE OF project_id, parent_goal_branch_id, parent_session_id
    ON goal_branch_proposals
    FOR EACH ROW EXECUTE FUNCTION goal_validate_proposal_parent_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contract_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  previous_project uuid;
  previous_branch uuid;
BEGIN
  SELECT project_id INTO branch_project FROM goal_branches WHERE id = NEW.goal_branch_id;
  IF branch_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'goal contract belongs to another project'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.supersedes_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO previous_project, previous_branch
    FROM goal_contract_versions WHERE id = NEW.supersedes_id;
    IF previous_project IS DISTINCT FROM NEW.project_id
       OR previous_branch IS DISTINCT FROM NEW.goal_branch_id THEN
      RAISE EXCEPTION 'superseded contract crosses aggregate scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contract_versions_scope
    AFTER INSERT ON goal_contract_versions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contract_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contract_revision_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  base_project uuid;
  base_branch uuid;
  proposed_project uuid;
  proposed_branch uuid;
  proposed_supersedes uuid;
  session_project uuid;
  session_branch uuid;
BEGIN
  SELECT project_id INTO branch_project FROM goal_branches WHERE id = NEW.goal_branch_id;
  SELECT project_id, goal_branch_id INTO base_project, base_branch
  FROM goal_contract_versions WHERE id = NEW.based_on_contract_version_id;
  SELECT project_id, goal_branch_id, supersedes_id
    INTO proposed_project, proposed_branch, proposed_supersedes
  FROM goal_contract_versions WHERE id = NEW.proposed_contract_version_id;
  IF branch_project IS DISTINCT FROM NEW.project_id
     OR base_project IS DISTINCT FROM NEW.project_id
     OR proposed_project IS DISTINCT FROM NEW.project_id
     OR base_branch IS DISTINCT FROM NEW.goal_branch_id
     OR proposed_branch IS DISTINCT FROM NEW.goal_branch_id
     OR proposed_supersedes IS DISTINCT FROM NEW.based_on_contract_version_id THEN
    RAISE EXCEPTION 'contract revision request crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.proposed_by_session_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO session_project, session_branch
    FROM goal_sessions WHERE id = NEW.proposed_by_session_id;
    IF session_project IS DISTINCT FROM NEW.project_id
       OR session_branch IS DISTINCT FROM NEW.goal_branch_id THEN
      RAISE EXCEPTION 'contract revision Session crosses aggregate scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contract_revision_requests_scope
    AFTER INSERT ON goal_contract_revision_requests
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contract_revision_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_contract_revision_request()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'contract revision request is an immutable audit record'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.based_on_contract_version_id IS DISTINCT FROM NEW.based_on_contract_version_id
     OR OLD.proposed_contract_version_id IS DISTINCT FROM NEW.proposed_contract_version_id
     OR OLD.proposed_by_session_id IS DISTINCT FROM NEW.proposed_by_session_id
     OR OLD.reason IS DISTINCT FROM NEW.reason
     OR OLD.change_summary IS DISTINCT FROM NEW.change_summary
     OR OLD.created_by IS DISTINCT FROM NEW.created_by
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'contract revision proposal identity is immutable'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.status <> 'awaiting_approval' OR NEW.status NOT IN ('accepted', 'rejected') THEN
    RAISE EXCEPTION 'invalid contract revision status transition'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS goal_contract_revision_requests_guard
  ON goal_contract_revision_requests;
CREATE TRIGGER goal_contract_revision_requests_guard
  BEFORE UPDATE OR DELETE ON goal_contract_revision_requests
  FOR EACH ROW EXECUTE FUNCTION goal_guard_contract_revision_request();

CREATE OR REPLACE FUNCTION goal_require_contract_revision_decision()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW.status IN ('accepted', 'rejected') AND NOT EXISTS (
    SELECT 1 FROM goal_contract_revision_decisions decision
    WHERE decision.revision_request_id = NEW.id
      AND decision.actor_role = 'human'
      AND decision.decision = CASE NEW.status
        WHEN 'accepted' THEN 'accept'
        ELSE 'reject'
      END
  ) THEN
    RAISE EXCEPTION 'terminal contract revision requires a matching human decision'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE CONSTRAINT TRIGGER goal_contract_revision_decision_required
    AFTER UPDATE OF status ON goal_contract_revision_requests
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION goal_require_contract_revision_decision();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contract_revision_decision_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  request_project uuid;
  request_status text;
BEGIN
  SELECT project_id, status INTO request_project, request_status
  FROM goal_contract_revision_requests WHERE id = NEW.revision_request_id;
  IF request_project IS DISTINCT FROM NEW.project_id
     OR (NEW.decision = 'accept' AND request_status IS DISTINCT FROM 'accepted')
     OR (NEW.decision = 'reject' AND request_status IS DISTINCT FROM 'rejected') THEN
    RAISE EXCEPTION 'contract revision decision does not match request state'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE CONSTRAINT TRIGGER goal_contract_revision_decisions_scope
    AFTER INSERT ON goal_contract_revision_decisions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contract_revision_decision_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contract_provenance_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  contract_project uuid;
  contract_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO contract_project, contract_branch
  FROM goal_contract_versions WHERE id = NEW.contract_version_id;
  IF contract_project IS DISTINCT FROM NEW.project_id
     OR contract_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'contract provenance crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contract_provenance_scope
    AFTER INSERT ON goal_contract_provenance
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contract_provenance_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_evidence_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  artifact_project uuid;
  tool_project uuid;
  tool_branch uuid;
  tool_session uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO session_project, session_branch
  FROM goal_sessions WHERE id = NEW.session_id;
  IF session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'Evidence Session crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.artifact_id IS NOT NULL THEN
    SELECT project_id INTO artifact_project FROM artifacts WHERE id = NEW.artifact_id;
    IF artifact_project IS DISTINCT FROM NEW.project_id THEN
      RAISE EXCEPTION 'Evidence Artifact crosses project scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NEW.tool_call_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id, session_id INTO tool_project, tool_branch, tool_session
    FROM tool_calls WHERE id = NEW.tool_call_id;
    IF tool_project IS DISTINCT FROM NEW.project_id
       OR tool_branch IS DISTINCT FROM NEW.goal_branch_id
       OR tool_session IS DISTINCT FROM NEW.session_id THEN
      RAISE EXCEPTION 'Evidence ToolCall crosses Session scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_evidence_scope
    AFTER INSERT ON goal_evidence
    FOR EACH ROW EXECUTE FUNCTION goal_validate_evidence_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contribution_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  previous_project uuid;
  previous_branch uuid;
  artifact_project uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO session_project, session_branch
  FROM goal_sessions WHERE id = NEW.session_id;
  IF session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'Contribution Session crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.supersedes_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO previous_project, previous_branch
    FROM goal_contributions WHERE id = NEW.supersedes_id;
    IF previous_project IS DISTINCT FROM NEW.project_id
       OR previous_branch IS DISTINCT FROM NEW.goal_branch_id THEN
      RAISE EXCEPTION 'superseded Contribution crosses aggregate scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NEW.artifact_id IS NOT NULL THEN
    SELECT project_id INTO artifact_project FROM artifacts WHERE id = NEW.artifact_id;
    IF artifact_project IS DISTINCT FROM NEW.project_id THEN
      RAISE EXCEPTION 'Contribution Artifact crosses project scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contributions_scope
    AFTER INSERT ON goal_contributions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contribution_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_contribution_evidence_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  contribution_project uuid;
  contribution_branch uuid;
  evidence_project uuid;
  evidence_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO contribution_project, contribution_branch
  FROM goal_contributions WHERE id = NEW.contribution_id;
  SELECT project_id, goal_branch_id INTO evidence_project, evidence_branch
  FROM goal_evidence WHERE id = NEW.evidence_id;
  IF contribution_project IS DISTINCT FROM evidence_project
     OR contribution_branch IS DISTINCT FROM evidence_branch THEN
    RAISE EXCEPTION 'Contribution Evidence crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contribution_evidence_scope
    AFTER INSERT ON goal_contribution_evidence
    FOR EACH ROW EXECUTE FUNCTION goal_validate_contribution_evidence_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_review_gate_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  contract_project uuid;
  contract_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO session_project, session_branch
  FROM goal_sessions WHERE id = NEW.session_id;
  SELECT project_id, goal_branch_id INTO contract_project, contract_branch
  FROM goal_contract_versions WHERE id = NEW.contract_version_id;
  IF session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id
     OR contract_project IS DISTINCT FROM NEW.project_id
     OR contract_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'ReviewGate crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_review_gates_scope
    AFTER INSERT ON goal_review_gates
    FOR EACH ROW EXECUTE FUNCTION goal_validate_review_gate_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_review_gate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    IF NEW.status <> 'pending_ai_review' THEN
      RAISE EXCEPTION 'ReviewGate must start at independent AI review'
        USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
  END IF;
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'ReviewGate is an immutable audit record'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.contract_version_id IS DISTINCT FROM NEW.contract_version_id
     OR OLD.candidate_snapshot IS DISTINCT FROM NEW.candidate_snapshot
     OR OLD.candidate_hash IS DISTINCT FROM NEW.candidate_hash
     OR OLD.git_base_commit IS DISTINCT FROM NEW.git_base_commit
     OR OLD.git_head_commit IS DISTINCT FROM NEW.git_head_commit
     OR OLD.git_dirty IS DISTINCT FROM NEW.git_dirty
     OR OLD.environment_fingerprint IS DISTINCT FROM NEW.environment_fingerprint
     OR OLD.test_evidence IS DISTINCT FROM NEW.test_evidence
     OR OLD.risks IS DISTINCT FROM NEW.risks
     OR OLD.self_check IS DISTINCT FROM NEW.self_check
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'frozen ReviewGate identity cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF NOT (
    (OLD.status = 'pending_ai_review'
      AND NEW.status IN ('pending_human_review', 'withdrawn'))
    OR (OLD.status = 'pending_human_review'
      AND NEW.status IN (
        'accepted', 'partially_accepted', 'rejected', 'abandoned', 'withdrawn'
      ))
  ) THEN
    RAISE EXCEPTION 'invalid ReviewGate status transition'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS goal_review_gates_guard ON goal_review_gates;
CREATE TRIGGER goal_review_gates_guard
  BEFORE INSERT OR UPDATE OR DELETE ON goal_review_gates
  FOR EACH ROW EXECUTE FUNCTION goal_guard_review_gate();

CREATE OR REPLACE FUNCTION goal_validate_gate_contribution_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  gate_project uuid;
  gate_branch uuid;
  contribution_project uuid;
  contribution_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO gate_project, gate_branch
  FROM goal_review_gates WHERE id = NEW.review_gate_id;
  SELECT project_id, goal_branch_id INTO contribution_project, contribution_branch
  FROM goal_contributions WHERE id = NEW.contribution_id;
  IF gate_project IS DISTINCT FROM contribution_project
     OR gate_branch IS DISTINCT FROM contribution_branch THEN
    RAISE EXCEPTION 'ReviewGate Contribution crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_review_gate_contributions_scope
    AFTER INSERT ON goal_review_gate_contributions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_gate_contribution_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_gate_evidence_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  gate_project uuid;
  gate_branch uuid;
  evidence_project uuid;
  evidence_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO gate_project, gate_branch
  FROM goal_review_gates WHERE id = NEW.review_gate_id;
  SELECT project_id, goal_branch_id INTO evidence_project, evidence_branch
  FROM goal_evidence WHERE id = NEW.evidence_id;
  IF gate_project IS DISTINCT FROM evidence_project
     OR gate_branch IS DISTINCT FROM evidence_branch THEN
    RAISE EXCEPTION 'ReviewGate Evidence crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_review_gate_evidence_scope
    AFTER INSERT ON goal_review_gate_evidence
    FOR EACH ROW EXECUTE FUNCTION goal_validate_gate_evidence_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_review_decision_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  gate_project uuid;
BEGIN
  SELECT project_id INTO gate_project FROM goal_review_gates WHERE id = NEW.review_gate_id;
  IF gate_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'ReviewDecision crosses project scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_review_decisions_scope
    AFTER INSERT ON goal_review_decisions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_review_decision_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_integration_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  source_project uuid;
  target_project uuid;
  gate_project uuid;
  gate_branch uuid;
BEGIN
  SELECT project_id INTO source_project FROM goal_branches WHERE id = NEW.source_goal_branch_id;
  IF NEW.target_goal_branch_id IS NOT NULL THEN
    SELECT project_id INTO target_project FROM goal_branches WHERE id = NEW.target_goal_branch_id;
  END IF;
  SELECT project_id, goal_branch_id INTO gate_project, gate_branch
  FROM goal_review_gates WHERE id = NEW.review_gate_id;
  IF source_project IS DISTINCT FROM NEW.project_id
     OR (NEW.target_goal_branch_id IS NOT NULL AND target_project IS DISTINCT FROM NEW.project_id)
     OR gate_project IS DISTINCT FROM NEW.project_id
     OR gate_branch IS DISTINCT FROM NEW.source_goal_branch_id THEN
    RAISE EXCEPTION 'GoalIntegration crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_integrations_scope
    AFTER INSERT ON goal_integrations
    FOR EACH ROW EXECUTE FUNCTION goal_validate_integration_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_integration_contribution_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  source_branch uuid;
  contribution_branch uuid;
BEGIN
  SELECT source_goal_branch_id INTO source_branch
  FROM goal_integrations WHERE id = NEW.integration_id;
  SELECT goal_branch_id INTO contribution_branch
  FROM goal_contributions WHERE id = NEW.contribution_id;
  IF source_branch IS DISTINCT FROM contribution_branch THEN
    RAISE EXCEPTION 'integrated Contribution belongs to another branch'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_integration_contributions_scope
    AFTER INSERT ON goal_integration_contributions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_integration_contribution_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_attention_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  session_project uuid;
  session_branch uuid;
BEGIN
  IF NEW.goal_branch_id IS NOT NULL THEN
    SELECT project_id INTO branch_project FROM goal_branches WHERE id = NEW.goal_branch_id;
    IF branch_project IS DISTINCT FROM NEW.project_id THEN
      RAISE EXCEPTION 'AttentionItem branch crosses project scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NEW.session_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO session_project, session_branch
    FROM goal_sessions WHERE id = NEW.session_id;
    IF session_project IS DISTINCT FROM NEW.project_id
       OR (NEW.goal_branch_id IS NOT NULL AND session_branch IS DISTINCT FROM NEW.goal_branch_id) THEN
      RAISE EXCEPTION 'AttentionItem Session crosses aggregate scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_attention_items_scope
    AFTER INSERT OR UPDATE OF project_id, goal_branch_id, session_id
    ON goal_attention_items
    FOR EACH ROW EXECUTE FUNCTION goal_validate_attention_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_branch_archive()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    IF NEW.status = 'archived' OR NEW.archived_from_status IS NOT NULL THEN
      RAISE EXCEPTION 'GoalBranch cannot be created as an archived conclusion'
        USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
  END IF;
  IF OLD.status = 'archived' AND (
    NEW.status IS DISTINCT FROM OLD.status
    OR NEW.archived_from_status IS DISTINCT FROM OLD.archived_from_status
  ) THEN
    RAISE EXCEPTION 'archived GoalBranch conclusion cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF NEW.status = 'archived' AND (
    OLD.status NOT IN ('integrated', 'completed', 'stopped')
    OR NEW.archived_from_status IS DISTINCT FROM OLD.status
  ) THEN
    RAISE EXCEPTION 'GoalBranch can only archive an exact terminal conclusion'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.status <> 'archived' AND NEW.archived_from_status IS NOT NULL THEN
    RAISE EXCEPTION 'non-archived GoalBranch cannot carry an archived conclusion'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS goal_branches_archive_guard ON goal_branches;
CREATE TRIGGER goal_branches_archive_guard
  BEFORE INSERT OR UPDATE OF status, archived_from_status ON goal_branches
  FOR EACH ROW EXECUTE FUNCTION goal_guard_branch_archive();

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'goal_contract_revision_decisions',
    'goal_contract_provenance',
    'goal_evidence',
    'goal_contribution_evidence',
    'goal_review_gate_evidence'
  ]
  LOOP
    trigger_name := table_name || '_immutable';
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = trigger_name AND tgrelid = table_name::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I '
        'FOR EACH ROW EXECUTE FUNCTION goal_reject_immutable_mutation()',
        trigger_name,
        table_name
      );
    END IF;
  END LOOP;
END;
$$;
