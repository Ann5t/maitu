ALTER TABLE goal_review_gates
  ADD COLUMN IF NOT EXISTS workspace_id uuid REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS tree_id text CHECK (
    tree_id IS NULL OR (tree_id ~ '^[0-9a-f]+$' AND length(tree_id) IN (40, 64))
  ),
  ADD COLUMN IF NOT EXISTS workspace_snapshot text CHECK (
    workspace_snapshot IS NULL OR workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS frozen_material jsonb CHECK (
    frozen_material IS NULL OR jsonb_typeof(frozen_material) = 'object'
  ),
  ADD COLUMN IF NOT EXISTS frozen_candidate_digest text CHECK (
    frozen_candidate_digest IS NULL OR frozen_candidate_digest ~ '^sha256:[0-9a-f]{64}$'
  );

ALTER TABLE goal_review_decisions
  ADD COLUMN IF NOT EXISTS action_run_id uuid REFERENCES goal_action_runs(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS action_lease_id uuid REFERENCES action_run_leases(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS worker_id uuid REFERENCES scheduler_workers(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS candidate_digest text CHECK (
    candidate_digest IS NULL OR candidate_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS report_digest text CHECK (
    report_digest IS NULL OR report_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS observed_snapshot jsonb CHECK (
    observed_snapshot IS NULL OR jsonb_typeof(observed_snapshot) = 'object'
  ),
  ADD COLUMN IF NOT EXISTS counterexamples jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (
    jsonb_typeof(counterexamples) = 'array'
  );

CREATE UNIQUE INDEX IF NOT EXISTS goal_review_decisions_action_run_idx
  ON goal_review_decisions (action_run_id) WHERE action_run_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS goal_review_decisions_report_digest_idx
  ON goal_review_decisions (report_digest) WHERE report_digest IS NOT NULL;

ALTER TABLE goal_integrations
  DROP CONSTRAINT IF EXISTS goal_integrations_git_integration_status_check;
ALTER TABLE goal_integrations
  ADD CONSTRAINT goal_integrations_git_integration_status_check CHECK (
    git_integration_status IN (
      'not_attempted', 'pending', 'preparing', 'validating', 'applying',
      'applied', 'conflicted', 'failed'
    )
  );

ALTER TABLE goal_integrations
  ADD COLUMN IF NOT EXISTS source_workspace_id uuid REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS target_workspace_id uuid REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS operation_id uuid REFERENCES workspace_operations(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS action_run_id uuid REFERENCES goal_action_runs(id) ON DELETE RESTRICT,
  ADD COLUMN IF NOT EXISTS source_head_commit text CHECK (
    source_head_commit IS NULL OR (
      source_head_commit ~ '^[0-9a-f]+$' AND length(source_head_commit) IN (40, 64)
    )
  ),
  ADD COLUMN IF NOT EXISTS source_tree_id text CHECK (
    source_tree_id IS NULL OR (
      source_tree_id ~ '^[0-9a-f]+$' AND length(source_tree_id) IN (40, 64)
    )
  ),
  ADD COLUMN IF NOT EXISTS source_workspace_snapshot text CHECK (
    source_workspace_snapshot IS NULL OR source_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS source_candidate_digest text CHECK (
    source_candidate_digest IS NULL OR source_candidate_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS expected_target_head_commit text CHECK (
    expected_target_head_commit IS NULL OR (
      expected_target_head_commit ~ '^[0-9a-f]+$'
      AND length(expected_target_head_commit) IN (40, 64)
    )
  ),
  ADD COLUMN IF NOT EXISTS expected_target_workspace_snapshot text CHECK (
    expected_target_workspace_snapshot IS NULL
    OR expected_target_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS selected_commits jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (
    jsonb_typeof(selected_commits) = 'array'
  ),
  ADD COLUMN IF NOT EXISTS preparation_key text CHECK (
    preparation_key IS NULL OR preparation_key ~ '^integrations/[0-9a-f-]{36}$'
  ),
  ADD COLUMN IF NOT EXISTS prepared_fencing_token bigint CHECK (
    prepared_fencing_token IS NULL OR prepared_fencing_token > 0
  ),
  ADD COLUMN IF NOT EXISTS candidate_commit text CHECK (
    candidate_commit IS NULL OR (
      candidate_commit ~ '^[0-9a-f]+$' AND length(candidate_commit) IN (40, 64)
    )
  ),
  ADD COLUMN IF NOT EXISTS candidate_tree_id text CHECK (
    candidate_tree_id IS NULL OR (
      candidate_tree_id ~ '^[0-9a-f]+$' AND length(candidate_tree_id) IN (40, 64)
    )
  ),
  ADD COLUMN IF NOT EXISTS candidate_workspace_snapshot text CHECK (
    candidate_workspace_snapshot IS NULL
    OR candidate_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS validation_report jsonb CHECK (
    validation_report IS NULL OR jsonb_typeof(validation_report) = 'object'
  ),
  ADD COLUMN IF NOT EXISTS validation_report_digest text CHECK (
    validation_report_digest IS NULL OR validation_report_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  ADD COLUMN IF NOT EXISTS last_error_code text,
  ADD COLUMN IF NOT EXISTS last_error_summary text,
  ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now(),
  ADD COLUMN IF NOT EXISTS applied_at timestamptz;

CREATE UNIQUE INDEX IF NOT EXISTS goal_integrations_action_run_idx
  ON goal_integrations (action_run_id) WHERE action_run_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS goal_integrations_operation_idx
  ON goal_integrations (operation_id) WHERE operation_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS goal_integrations_preparation_key_idx
  ON goal_integrations (preparation_key) WHERE preparation_key IS NOT NULL;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_review_gates_physical_binding_shape'
      AND conrelid = 'goal_review_gates'::regclass
  ) THEN
    ALTER TABLE goal_review_gates
      ADD CONSTRAINT goal_review_gates_physical_binding_shape CHECK (
        (workspace_id IS NULL AND tree_id IS NULL AND workspace_snapshot IS NULL
          AND frozen_material IS NULL AND frozen_candidate_digest IS NULL)
        OR
        (workspace_id IS NOT NULL AND git_base_commit IS NOT NULL
          AND git_head_commit IS NOT NULL AND git_dirty = false
          AND tree_id IS NOT NULL AND workspace_snapshot IS NOT NULL
          AND frozen_material IS NOT NULL AND frozen_candidate_digest IS NOT NULL)
      );
  END IF;
END $$;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_review_decisions_worker_binding_shape'
      AND conrelid = 'goal_review_decisions'::regclass
  ) THEN
    ALTER TABLE goal_review_decisions
      ADD CONSTRAINT goal_review_decisions_worker_binding_shape CHECK (
        (action_run_id IS NULL AND action_lease_id IS NULL AND worker_id IS NULL
          AND candidate_digest IS NULL AND report_digest IS NULL
          AND observed_snapshot IS NULL)
        OR
        (actor_role = 'review_ai' AND action_run_id IS NOT NULL
          AND action_lease_id IS NOT NULL AND worker_id IS NOT NULL
          AND candidate_digest IS NOT NULL AND report_digest IS NOT NULL
          AND observed_snapshot IS NOT NULL)
      );
  END IF;
END $$;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_integrations_physical_binding_shape'
      AND conrelid = 'goal_integrations'::regclass
  ) THEN
    ALTER TABLE goal_integrations
      ADD CONSTRAINT goal_integrations_physical_binding_shape CHECK (
        (source_workspace_id IS NULL AND target_workspace_id IS NULL
          AND operation_id IS NULL AND action_run_id IS NULL
          AND source_head_commit IS NULL AND source_tree_id IS NULL
          AND source_workspace_snapshot IS NULL AND source_candidate_digest IS NULL
          AND expected_target_head_commit IS NULL
          AND expected_target_workspace_snapshot IS NULL
          AND preparation_key IS NULL AND prepared_fencing_token IS NULL
          AND candidate_commit IS NULL AND candidate_tree_id IS NULL
          AND candidate_workspace_snapshot IS NULL AND validation_report IS NULL
          AND validation_report_digest IS NULL AND applied_at IS NULL)
        OR
        (source_workspace_id IS NOT NULL AND target_workspace_id IS NOT NULL
          AND target_goal_branch_id IS NOT NULL AND operation_id IS NOT NULL
          AND source_head_commit IS NOT NULL AND source_tree_id IS NOT NULL
          AND source_workspace_snapshot IS NOT NULL AND source_candidate_digest IS NOT NULL
          AND expected_target_head_commit IS NOT NULL
          AND expected_target_workspace_snapshot IS NOT NULL
          AND git_integration_status <> 'not_attempted')
      );
  END IF;
END $$;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_integrations_candidate_group_shape'
      AND conrelid = 'goal_integrations'::regclass
  ) THEN
    ALTER TABLE goal_integrations
      ADD CONSTRAINT goal_integrations_candidate_group_shape CHECK (
        (preparation_key IS NULL AND candidate_commit IS NULL
          AND candidate_tree_id IS NULL AND candidate_workspace_snapshot IS NULL)
        OR
        (preparation_key IS NOT NULL AND candidate_commit IS NOT NULL
          AND candidate_tree_id IS NOT NULL AND candidate_workspace_snapshot IS NOT NULL)
      );
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_integrations_validation_group_shape'
      AND conrelid = 'goal_integrations'::regclass
  ) THEN
    ALTER TABLE goal_integrations
      ADD CONSTRAINT goal_integrations_validation_group_shape CHECK (
        (validation_report IS NULL AND validation_report_digest IS NULL)
        OR (validation_report IS NOT NULL AND validation_report_digest IS NOT NULL)
      );
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_integrations_terminal_shape'
      AND conrelid = 'goal_integrations'::regclass
  ) THEN
    ALTER TABLE goal_integrations
      ADD CONSTRAINT goal_integrations_terminal_shape CHECK (
        git_integration_status <> 'applied'
        OR source_workspace_id IS NULL
        OR (candidate_commit IS NOT NULL AND candidate_tree_id IS NOT NULL
          AND candidate_workspace_snapshot IS NOT NULL
          AND validation_report IS NOT NULL AND validation_report_digest IS NOT NULL
          AND applied_at IS NOT NULL)
      );
  END IF;
END $$;

ALTER TABLE workspace_operations
  DROP CONSTRAINT IF EXISTS workspace_operations_operation_kind_check;
ALTER TABLE workspace_operations
  ADD CONSTRAINT workspace_operations_operation_kind_check CHECK (
    operation_kind IN ('provision', 'apply', 'reconcile', 'integration')
  );

ALTER TABLE goal_attention_items
  DROP CONSTRAINT IF EXISTS goal_attention_items_kind_check;
ALTER TABLE goal_attention_items
  ADD CONSTRAINT goal_attention_items_kind_check CHECK (kind IN (
    'branch_review', 'dependency', 'judgment', 'merge_review', 'exception',
    'manual_pause', 'continuation_required', 'child_result_ready', 'contract_review',
    'action_run_failure', 'integration_conflict'
  ));

ALTER TABLE goal_notifications
  DROP CONSTRAINT IF EXISTS goal_notifications_kind_check;
ALTER TABLE goal_notifications
  ADD CONSTRAINT goal_notifications_kind_check CHECK (kind IN (
    'action_waiting', 'action_failed', 'action_timed_out', 'tool_lease_expired',
    'review_failed', 'integration_conflict', 'integration_failed'
  ));

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
     OR OLD.workspace_id IS DISTINCT FROM NEW.workspace_id
     OR OLD.tree_id IS DISTINCT FROM NEW.tree_id
     OR OLD.workspace_snapshot IS DISTINCT FROM NEW.workspace_snapshot
     OR OLD.frozen_material IS DISTINCT FROM NEW.frozen_material
     OR OLD.frozen_candidate_digest IS DISTINCT FROM NEW.frozen_candidate_digest
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

CREATE OR REPLACE FUNCTION goal_guard_integration()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    IF NEW.git_integration_status NOT IN ('not_attempted', 'pending') THEN
      RAISE EXCEPTION 'GoalIntegration must start pending'
        USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
  END IF;
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'GoalIntegration is an immutable audit record'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.source_goal_branch_id IS DISTINCT FROM NEW.source_goal_branch_id
     OR OLD.target_goal_branch_id IS DISTINCT FROM NEW.target_goal_branch_id
     OR OLD.review_gate_id IS DISTINCT FROM NEW.review_gate_id
     OR OLD.kind IS DISTINCT FROM NEW.kind
     OR OLD.summary IS DISTINCT FROM NEW.summary
     OR OLD.source_workspace_id IS DISTINCT FROM NEW.source_workspace_id
     OR OLD.target_workspace_id IS DISTINCT FROM NEW.target_workspace_id
     OR OLD.operation_id IS DISTINCT FROM NEW.operation_id
     OR OLD.source_head_commit IS DISTINCT FROM NEW.source_head_commit
     OR OLD.source_tree_id IS DISTINCT FROM NEW.source_tree_id
     OR OLD.source_workspace_snapshot IS DISTINCT FROM NEW.source_workspace_snapshot
     OR OLD.source_candidate_digest IS DISTINCT FROM NEW.source_candidate_digest
     OR OLD.expected_target_head_commit IS DISTINCT FROM NEW.expected_target_head_commit
     OR OLD.expected_target_workspace_snapshot IS DISTINCT FROM NEW.expected_target_workspace_snapshot
     OR OLD.selected_commits IS DISTINCT FROM NEW.selected_commits
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'GoalIntegration frozen identity cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.action_run_id IS NOT NULL
     AND OLD.action_run_id IS DISTINCT FROM NEW.action_run_id THEN
    RAISE EXCEPTION 'GoalIntegration ActionRun cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.candidate_commit IS NOT NULL AND (
       OLD.candidate_commit IS DISTINCT FROM NEW.candidate_commit
       OR OLD.candidate_tree_id IS DISTINCT FROM NEW.candidate_tree_id
       OR OLD.candidate_workspace_snapshot IS DISTINCT FROM NEW.candidate_workspace_snapshot
       OR OLD.preparation_key IS DISTINCT FROM NEW.preparation_key
     ) THEN
    RAISE EXCEPTION 'prepared integration candidate cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.validation_report IS NOT NULL AND (
       OLD.validation_report IS DISTINCT FROM NEW.validation_report
       OR OLD.validation_report_digest IS DISTINCT FROM NEW.validation_report_digest
     ) THEN
    RAISE EXCEPTION 'integration validation report cannot change'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.git_integration_status = 'applied' THEN
    RAISE EXCEPTION 'applied GoalIntegration is immutable'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.git_integration_status IS DISTINCT FROM NEW.git_integration_status AND NOT (
       (OLD.git_integration_status = 'not_attempted' AND NEW.git_integration_status = 'pending')
       OR (OLD.git_integration_status = 'pending'
           AND NEW.git_integration_status IN ('preparing', 'conflicted', 'failed'))
       OR (OLD.git_integration_status = 'preparing'
           AND NEW.git_integration_status IN ('validating', 'conflicted', 'failed'))
       OR (OLD.git_integration_status = 'validating'
           AND NEW.git_integration_status IN ('applying', 'conflicted', 'failed', 'preparing'))
       OR (OLD.git_integration_status = 'applying'
           AND NEW.git_integration_status IN ('applied', 'conflicted', 'failed'))
       OR (OLD.git_integration_status = 'conflicted'
           AND NEW.git_integration_status IN ('preparing', 'failed'))
     ) THEN
    RAISE EXCEPTION 'invalid GoalIntegration status transition'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

-- 0002 treated GoalIntegration as a write-once logical receipt.  The physical
-- integration protocol introduced here is a guarded state machine, so replace
-- that coarse immutable trigger with the field- and transition-aware guard
-- below.  Terminal `applied` records remain immutable.
DROP TRIGGER IF EXISTS goal_integrations_immutable ON goal_integrations;
DROP TRIGGER IF EXISTS goal_integrations_guard ON goal_integrations;
CREATE TRIGGER goal_integrations_guard
  BEFORE INSERT OR UPDATE OR DELETE ON goal_integrations
  FOR EACH ROW EXECUTE FUNCTION goal_guard_integration();

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'goal_contributions', 'goal_review_decisions',
    'goal_review_gate_contributions', 'goal_integration_contributions'
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
