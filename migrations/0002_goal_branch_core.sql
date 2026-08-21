CREATE TABLE IF NOT EXISTS goal_branch_proposals (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  parent_goal_branch_id uuid,
  parent_session_id uuid,
  status text NOT NULL DEFAULT 'draft' CHECK (status IN (
    'draft', 'awaiting_approval', 'approved', 'cancelled'
  )),
  current_revision integer NOT NULL DEFAULT 1 CHECK (current_revision > 0),
  approved_revision integer CHECK (approved_revision IS NULL OR approved_revision > 0),
  approved_goal_branch_id uuid UNIQUE,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  CHECK (
    (parent_goal_branch_id IS NULL AND parent_session_id IS NULL)
    OR (parent_goal_branch_id IS NOT NULL AND parent_session_id IS NOT NULL)
  ),
  CHECK (
    (status = 'approved' AND approved_revision IS NOT NULL AND approved_goal_branch_id IS NOT NULL)
    OR (status <> 'approved' AND approved_goal_branch_id IS NULL)
  )
);

CREATE TABLE IF NOT EXISTS goal_branch_proposal_revisions (
  id uuid NOT NULL UNIQUE,
  proposal_id uuid NOT NULL REFERENCES goal_branch_proposals(id) ON DELETE CASCADE,
  revision integer NOT NULL CHECK (revision > 0),
  why_needed text NOT NULL,
  contract jsonb NOT NULL CHECK (jsonb_typeof(contract) = 'object'),
  expected_contributions jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(expected_contributions) = 'array'),
  exploration_plan jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(exploration_plan) = 'array'),
  context_inheritance jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(context_inheritance) = 'object'),
  tool_requirements jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(tool_requirements) = 'array'),
  inferences jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(inferences) = 'array'),
  revision_reason text,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (proposal_id, revision)
);

DO $$ BEGIN
  ALTER TABLE goal_branch_proposals
    ADD CONSTRAINT goal_branch_proposals_current_revision_fk
    FOREIGN KEY (id, current_revision)
    REFERENCES goal_branch_proposal_revisions(proposal_id, revision)
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branch_proposals
    ADD CONSTRAINT goal_branch_proposals_approved_revision_fk
    FOREIGN KEY (id, approved_revision)
    REFERENCES goal_branch_proposal_revisions(proposal_id, revision)
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS goal_branches (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  creating_proposal_id uuid NOT NULL UNIQUE
    REFERENCES goal_branch_proposals(id) ON DELETE RESTRICT,
  parent_goal_branch_id uuid REFERENCES goal_branches(id) ON DELETE RESTRICT,
  inherited_from_session_id uuid,
  name text NOT NULL,
  status text NOT NULL DEFAULT 'active' CHECK (status IN (
    'active', 'waiting', 'review_pending', 'integrated', 'completed', 'stopped', 'archived'
  )),
  current_contract_version_id uuid NOT NULL,
  head_session_id uuid NOT NULL,
  git_branch_name text,
  worktree_path text,
  base_commit text,
  environment_fingerprint text CHECK (
    environment_fingerprint IS NULL OR environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  stopped_at timestamptz,
  CHECK (
    (parent_goal_branch_id IS NULL AND inherited_from_session_id IS NULL)
    OR (parent_goal_branch_id IS NOT NULL AND inherited_from_session_id IS NOT NULL)
  ),
  UNIQUE (project_id, git_branch_name)
);

CREATE TABLE IF NOT EXISTS goal_contract_versions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  version integer NOT NULL CHECK (version > 0),
  desired_outcome text NOT NULL,
  hard_constraints jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(hard_constraints) = 'array'),
  subjective_preferences jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(subjective_preferences) = 'array'),
  unknowns jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(unknowns) = 'array'),
  non_goals jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(non_goals) = 'array'),
  validation_plan jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(validation_plan) = 'array'),
  judgment_triggers jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(judgment_triggers) = 'array'),
  stop_conditions jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(stop_conditions) = 'array'),
  expected_contributions jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(expected_contributions) = 'array'),
  source_proposal_id uuid REFERENCES goal_branch_proposals(id) ON DELETE RESTRICT,
  supersedes_id uuid REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (goal_branch_id, version)
);

DO $$ BEGIN
  ALTER TABLE goal_branches
    ADD CONSTRAINT goal_branches_current_contract_fk
    FOREIGN KEY (current_contract_version_id)
    REFERENCES goal_contract_versions(id) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS goal_sessions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  session_number integer NOT NULL CHECK (session_number > 0),
  status text NOT NULL DEFAULT 'running' CHECK (status IN (
    'running', 'waiting_branch_review', 'waiting_dependency', 'waiting_judgment',
    'exception_paused', 'manual_paused', 'awaiting_merge_review',
    'review_rejected', 'accepted', 'stopped'
  )),
  assignment text NOT NULL,
  agent_identity text,
  contract_version_id uuid NOT NULL REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  environment_fingerprint text CHECK (
    environment_fingerprint IS NULL OR environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  inherited_context jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(inherited_context) = 'object'),
  started_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  ended_at timestamptz,
  UNIQUE (goal_branch_id, session_number)
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_sessions_one_running_writer_idx
  ON goal_sessions (goal_branch_id) WHERE status = 'running';

CREATE OR REPLACE FUNCTION goal_validate_session_contract()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  contract_project_id uuid;
  contract_branch_id uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO contract_project_id, contract_branch_id
  FROM goal_contract_versions WHERE id = NEW.contract_version_id;
  IF contract_project_id IS DISTINCT FROM NEW.project_id
     OR contract_branch_id IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'goal Session contract belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_sessions_contract_scope
    AFTER INSERT OR UPDATE OF project_id, goal_branch_id, contract_version_id
    ON goal_sessions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_session_contract();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branches
    ADD CONSTRAINT goal_branches_head_session_fk
    FOREIGN KEY (head_session_id) REFERENCES goal_sessions(id) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branches
    ADD CONSTRAINT goal_branches_inherited_session_fk
    FOREIGN KEY (inherited_from_session_id) REFERENCES goal_sessions(id) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branch_proposals
    ADD CONSTRAINT goal_branch_proposals_parent_branch_fk
    FOREIGN KEY (parent_goal_branch_id) REFERENCES goal_branches(id) ON DELETE RESTRICT;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branch_proposals
    ADD CONSTRAINT goal_branch_proposals_parent_session_fk
    FOREIGN KEY (parent_session_id) REFERENCES goal_sessions(id) ON DELETE RESTRICT;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE goal_branch_proposals
    ADD CONSTRAINT goal_branch_proposals_approved_branch_fk
    FOREIGN KEY (approved_goal_branch_id) REFERENCES goal_branches(id) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_branch_origin()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  proposal_status text;
  proposal_project_id uuid;
  proposal_parent_branch_id uuid;
  proposal_parent_session_id uuid;
  proposal_approved_branch_id uuid;
  contract_project_id uuid;
  contract_branch_id uuid;
  session_project_id uuid;
  session_branch_id uuid;
BEGIN
  SELECT status, project_id, parent_goal_branch_id, parent_session_id, approved_goal_branch_id
    INTO proposal_status, proposal_project_id, proposal_parent_branch_id,
         proposal_parent_session_id, proposal_approved_branch_id
  FROM goal_branch_proposals
  WHERE id = NEW.creating_proposal_id;

  IF proposal_status IS DISTINCT FROM 'approved'
     OR proposal_approved_branch_id IS DISTINCT FROM NEW.id THEN
    RAISE EXCEPTION 'goal branch requires its approved BranchProposal'
      USING ERRCODE = '23514';
  END IF;
  IF proposal_project_id IS DISTINCT FROM NEW.project_id
     OR proposal_parent_branch_id IS DISTINCT FROM NEW.parent_goal_branch_id
     OR proposal_parent_session_id IS DISTINCT FROM NEW.inherited_from_session_id THEN
    RAISE EXCEPTION 'goal branch origin does not match its BranchProposal'
      USING ERRCODE = '23514';
  END IF;

  SELECT project_id, goal_branch_id INTO contract_project_id, contract_branch_id
  FROM goal_contract_versions WHERE id = NEW.current_contract_version_id;
  IF contract_project_id IS DISTINCT FROM NEW.project_id
     OR contract_branch_id IS DISTINCT FROM NEW.id THEN
    RAISE EXCEPTION 'goal branch current contract belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;

  SELECT project_id, goal_branch_id INTO session_project_id, session_branch_id
  FROM goal_sessions WHERE id = NEW.head_session_id;
  IF session_project_id IS DISTINCT FROM NEW.project_id
     OR session_branch_id IS DISTINCT FROM NEW.id THEN
    RAISE EXCEPTION 'goal branch head Session belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE CONSTRAINT TRIGGER goal_branches_approved_origin
    AFTER INSERT OR UPDATE OF creating_proposal_id, parent_goal_branch_id,
      inherited_from_session_id, current_contract_version_id, head_session_id
    ON goal_branches
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION goal_validate_branch_origin();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS goal_contributions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  kind text NOT NULL CHECK (kind IN (
    'artifact', 'finding', 'evidence', 'decision', 'condition', 'code_change', 'other'
  )),
  title text NOT NULL,
  body text NOT NULL,
  artifact_id uuid REFERENCES artifacts(id) ON DELETE SET NULL,
  evidence_refs jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(evidence_refs) = 'array'),
  supersedes_id uuid REFERENCES goal_contributions(id) ON DELETE RESTRICT,
  content_hash text NOT NULL CHECK (content_hash ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS goal_review_gates (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE CASCADE,
  session_id uuid NOT NULL UNIQUE REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  contract_version_id uuid NOT NULL REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  status text NOT NULL DEFAULT 'pending_ai_review' CHECK (status IN (
    'pending_ai_review', 'pending_human_review', 'accepted', 'partially_accepted',
    'rejected', 'abandoned', 'withdrawn'
  )),
  candidate_snapshot jsonb NOT NULL CHECK (jsonb_typeof(candidate_snapshot) = 'object'),
  candidate_hash text NOT NULL CHECK (candidate_hash ~ '^sha256:[0-9a-f]{64}$'),
  git_base_commit text,
  git_head_commit text,
  git_dirty boolean NOT NULL DEFAULT false,
  environment_fingerprint text CHECK (
    environment_fingerprint IS NULL OR environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  test_evidence jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(test_evidence) = 'array'),
  risks jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(risks) = 'array'),
  self_check jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(self_check) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  resolved_at timestamptz
);

CREATE TABLE IF NOT EXISTS goal_review_gate_contributions (
  review_gate_id uuid NOT NULL REFERENCES goal_review_gates(id) ON DELETE CASCADE,
  contribution_id uuid NOT NULL REFERENCES goal_contributions(id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (review_gate_id, contribution_id)
);

CREATE TABLE IF NOT EXISTS goal_review_decisions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  review_gate_id uuid NOT NULL REFERENCES goal_review_gates(id) ON DELETE CASCADE,
  actor_role text NOT NULL CHECK (actor_role IN ('review_ai', 'human')),
  actor_identity text,
  decision text NOT NULL CHECK (decision IN (
    'recommend_accept', 'recommend_reject', 'accept', 'partial_accept',
    'reject', 'abandon', 'withdraw'
  )),
  rationale text NOT NULL,
  contract_check jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(contract_check) = 'object'),
  retest_evidence jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(retest_evidence) = 'array'),
  selected_contribution_ids jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(selected_contribution_ids) = 'array'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (review_gate_id, actor_role)
);

CREATE TABLE IF NOT EXISTS goal_integrations (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  source_goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  target_goal_branch_id uuid REFERENCES goal_branches(id) ON DELETE RESTRICT,
  review_gate_id uuid NOT NULL UNIQUE REFERENCES goal_review_gates(id) ON DELETE RESTRICT,
  kind text NOT NULL CHECK (kind IN ('full', 'partial')),
  summary text NOT NULL,
  git_integration_status text NOT NULL DEFAULT 'not_attempted' CHECK (
    git_integration_status IN ('not_attempted', 'pending', 'applied', 'failed')
  ),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS goal_integration_contributions (
  integration_id uuid NOT NULL REFERENCES goal_integrations(id) ON DELETE CASCADE,
  contribution_id uuid NOT NULL REFERENCES goal_contributions(id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (integration_id, contribution_id)
);

CREATE TABLE IF NOT EXISTS goal_attention_items (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid REFERENCES goal_branches(id) ON DELETE CASCADE,
  session_id uuid REFERENCES goal_sessions(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN (
    'branch_review', 'dependency', 'judgment', 'merge_review', 'exception',
    'manual_pause', 'continuation_required', 'child_result_ready'
  )),
  status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved', 'cancelled')),
  dedupe_key text NOT NULL,
  title text NOT NULL,
  reason text NOT NULL,
  safe_checkpoint text,
  attempted text,
  risk text,
  user_action text,
  recommendation text,
  resolution text,
  created_at timestamptz NOT NULL DEFAULT now(),
  resolved_at timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_attention_items_open_dedupe_idx
  ON goal_attention_items (project_id, dedupe_key) WHERE status = 'open';

CREATE TABLE IF NOT EXISTS goal_command_receipts (
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  client_request_id uuid NOT NULL,
  command_kind text NOT NULL,
  input_hash text NOT NULL CHECK (input_hash ~ '^sha256:[0-9a-f]{64}$'),
  result jsonb NOT NULL CHECK (jsonb_typeof(result) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, client_request_id)
);

CREATE TABLE IF NOT EXISTS goal_events (
  id uuid PRIMARY KEY,
  sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  aggregate_type text NOT NULL CHECK (aggregate_type IN (
    'proposal', 'goal_branch', 'session', 'contract', 'review_gate',
    'integration', 'attention', 'tool', 'input_artifact', 'project'
  )),
  aggregate_id uuid NOT NULL,
  event_type text NOT NULL,
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'review_ai', 'system')),
  actor_identity text,
  client_request_id uuid NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(payload) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS goal_branch_proposals_project_idx
  ON goal_branch_proposals (project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS goal_branches_project_idx
  ON goal_branches (project_id, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS goal_branches_one_root_idx
  ON goal_branches (project_id) WHERE parent_goal_branch_id IS NULL;
CREATE INDEX IF NOT EXISTS goal_sessions_branch_idx
  ON goal_sessions (goal_branch_id, session_number);
CREATE INDEX IF NOT EXISTS goal_contributions_branch_idx
  ON goal_contributions (goal_branch_id, created_at);
CREATE INDEX IF NOT EXISTS goal_review_gates_project_status_idx
  ON goal_review_gates (project_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS goal_events_project_sequence_idx
  ON goal_events (project_id, sequence);
CREATE INDEX IF NOT EXISTS goal_attention_items_project_status_idx
  ON goal_attention_items (project_id, status, created_at DESC);

CREATE OR REPLACE FUNCTION goal_reject_immutable_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'immutable goal audit record cannot be %', lower(TG_OP)
    USING ERRCODE = '55000';
END;
$$;

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'goal_branch_proposal_revisions',
    'goal_contract_versions',
    'goal_contributions',
    'goal_review_gate_contributions',
    'goal_review_decisions',
    'goal_integrations',
    'goal_integration_contributions',
    'goal_command_receipts',
    'goal_events'
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
