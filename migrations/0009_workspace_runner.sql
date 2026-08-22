ALTER TABLE goal_branch_proposal_revisions
  ADD COLUMN IF NOT EXISTS capability_policy jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(capability_policy) = 'object');

CREATE TABLE IF NOT EXISTS project_git_repositories (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
  storage_key text NOT NULL UNIQUE CHECK (
    storage_key ~ '^projects/[0-9a-f-]{36}\.git$'
    AND storage_key NOT LIKE '%..%'
  ),
  default_branch text NOT NULL CHECK (default_branch ~ '^[A-Za-z0-9][A-Za-z0-9._/-]{0,199}$'),
  default_head_commit text NOT NULL CHECK (
    default_head_commit ~ '^[0-9a-f]+$'
    AND length(default_head_commit) IN (40, 64)
  ),
  object_format text NOT NULL CHECK (object_format IN ('sha1', 'sha256')),
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'error', 'retired')),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS goal_workspace_policies (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL UNIQUE REFERENCES goal_branches(id) ON DELETE RESTRICT,
  source_proposal_id uuid NOT NULL REFERENCES goal_branch_proposals(id) ON DELETE RESTRICT,
  source_proposal_revision integer NOT NULL CHECK (source_proposal_revision > 0),
  policy jsonb NOT NULL CHECK (jsonb_typeof(policy) = 'object'),
  policy_hash text NOT NULL CHECK (policy_hash ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (source_proposal_id, source_proposal_revision)
);

CREATE TABLE IF NOT EXISTS goal_workspaces (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL UNIQUE REFERENCES goal_branches(id) ON DELETE RESTRICT,
  repository_id uuid NOT NULL REFERENCES project_git_repositories(id) ON DELETE RESTRICT,
  git_branch_name text NOT NULL CHECK (git_branch_name ~ '^goal/[0-9a-f-]{36}$'),
  worktree_key text NOT NULL UNIQUE CHECK (
    worktree_key ~ '^projects/[0-9a-f-]{36}/goals/[0-9a-f-]{36}$'
    AND worktree_key NOT LIKE '%..%'
  ),
  base_commit text CHECK (
    base_commit IS NULL OR (
      base_commit ~ '^[0-9a-f]+$' AND length(base_commit) IN (40, 64)
    )
  ),
  head_commit text CHECK (
    head_commit IS NULL OR (
      head_commit ~ '^[0-9a-f]+$' AND length(head_commit) IN (40, 64)
    )
  ),
  tree_id text CHECK (
    tree_id IS NULL OR (tree_id ~ '^[0-9a-f]+$' AND length(tree_id) IN (40, 64))
  ),
  workspace_snapshot text CHECK (
    workspace_snapshot IS NULL OR workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  dirty boolean NOT NULL DEFAULT false,
  status text NOT NULL DEFAULT 'provisioning' CHECK (
    status IN ('provisioning', 'ready', 'applying', 'error', 'frozen', 'retired')
  ),
  fencing_counter bigint NOT NULL DEFAULT 0 CHECK (fencing_counter >= 0),
  last_error_code text,
  last_error_summary text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (
    (status = 'provisioning' AND head_commit IS NULL AND workspace_snapshot IS NULL)
    OR (status <> 'provisioning' AND head_commit IS NOT NULL AND workspace_snapshot IS NOT NULL)
    OR status = 'error'
  ),
  UNIQUE (repository_id, git_branch_name)
);

CREATE TABLE IF NOT EXISTS workspace_operations (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  runner_job_id uuid,
  operation_kind text NOT NULL CHECK (operation_kind IN ('provision', 'apply', 'reconcile')),
  status text NOT NULL CHECK (status IN ('planned', 'applying', 'applied', 'failed')),
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  expected_head_commit text CHECK (
    expected_head_commit IS NULL OR (
      expected_head_commit ~ '^[0-9a-f]+$' AND length(expected_head_commit) IN (40, 64)
    )
  ),
  candidate_commit text CHECK (
    candidate_commit IS NULL OR (
      candidate_commit ~ '^[0-9a-f]+$' AND length(candidate_commit) IN (40, 64)
    )
  ),
  detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
  error_code text,
  error_summary text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS workspace_operations_one_open_idx
  ON workspace_operations (workspace_id)
  WHERE status IN ('planned', 'applying');

CREATE TABLE IF NOT EXISTS workspace_snapshots (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  operation_id uuid NOT NULL REFERENCES workspace_operations(id) ON DELETE RESTRICT,
  runner_job_id uuid,
  parent_snapshot_id uuid REFERENCES workspace_snapshots(id) ON DELETE RESTRICT,
  head_commit text NOT NULL CHECK (
    head_commit ~ '^[0-9a-f]+$' AND length(head_commit) IN (40, 64)
  ),
  tree_id text NOT NULL CHECK (tree_id ~ '^[0-9a-f]+$' AND length(tree_id) IN (40, 64)),
  dirty boolean NOT NULL,
  snapshot_hash text NOT NULL CHECK (snapshot_hash ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (operation_id)
);

ALTER TABLE workspace_snapshots
  DROP CONSTRAINT IF EXISTS workspace_snapshots_workspace_id_snapshot_hash_key;
DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'workspace_snapshots_operation_id_key'
      AND conrelid = 'workspace_snapshots'::regclass
  ) THEN
    ALTER TABLE workspace_snapshots
      ADD CONSTRAINT workspace_snapshots_operation_id_key UNIQUE (operation_id);
  END IF;
END $$;
CREATE INDEX IF NOT EXISTS workspace_snapshots_physical_state_idx
  ON workspace_snapshots (workspace_id, snapshot_hash, created_at DESC);

CREATE TABLE IF NOT EXISTS workspace_write_leases (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  status text NOT NULL CHECK (status IN ('active', 'released', 'expired', 'cancelled', 'failed')),
  fencing_token bigint NOT NULL CHECK (fencing_token > 0),
  renewal_token_digest text NOT NULL CHECK (renewal_token_digest ~ '^sha256:[0-9a-f]{64}$'),
  base_commit text NOT NULL CHECK (
    base_commit ~ '^[0-9a-f]+$' AND length(base_commit) IN (40, 64)
  ),
  base_workspace_snapshot text NOT NULL CHECK (
    base_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  allowed_writes jsonb NOT NULL CHECK (jsonb_typeof(allowed_writes) = 'array'),
  delete_paths jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(delete_paths) = 'array'),
  capabilities jsonb NOT NULL CHECK (jsonb_typeof(capabilities) = 'object'),
  resource_policy jsonb NOT NULL CHECK (jsonb_typeof(resource_policy) = 'object'),
  output_key text NOT NULL UNIQUE CHECK (
    output_key ~ '^jobs/[0-9a-f-]{36}$' AND output_key NOT LIKE '%..%'
  ),
  acquired_at timestamptz NOT NULL DEFAULT now(),
  soft_expires_at timestamptz NOT NULL,
  hard_expires_at timestamptz NOT NULL,
  completed_at timestamptz,
  CHECK (soft_expires_at <= hard_expires_at),
  UNIQUE (project_id, client_request_id),
  UNIQUE (workspace_id, fencing_token)
);

ALTER TABLE workspace_write_leases
  ADD COLUMN IF NOT EXISTS delete_paths jsonb NOT NULL DEFAULT '[]'::jsonb;
DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'workspace_write_leases_delete_paths_check'
      AND conrelid = 'workspace_write_leases'::regclass
  ) THEN
    ALTER TABLE workspace_write_leases
      ADD CONSTRAINT workspace_write_leases_delete_paths_check
      CHECK (jsonb_typeof(delete_paths) = 'array');
  END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS workspace_write_leases_one_active_idx
  ON workspace_write_leases (workspace_id) WHERE status = 'active';

CREATE TABLE IF NOT EXISTS runner_jobs (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES goal_workspaces(id) ON DELETE RESTRICT,
  lease_id uuid NOT NULL UNIQUE REFERENCES workspace_write_leases(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  status text NOT NULL CHECK (status IN (
    'prepared', 'running', 'applying', 'succeeded', 'failed', 'timed_out',
    'policy_denied', 'workspace_conflict', 'cancelled'
  )),
  spec jsonb NOT NULL CHECK (jsonb_typeof(spec) = 'object'),
  spec_hash text NOT NULL CHECK (spec_hash ~ '^sha256:[0-9a-f]{64}$'),
  runtime_digest text NOT NULL CHECK (runtime_digest ~ '^sha256:[0-9a-f]{64}$'),
  result jsonb CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
  output_manifest_hash text CHECK (
    output_manifest_hash IS NULL OR output_manifest_hash ~ '^sha256:[0-9a-f]{64}$'
  ),
  candidate_commit text CHECK (
    candidate_commit IS NULL OR (
      candidate_commit ~ '^[0-9a-f]+$' AND length(candidate_commit) IN (40, 64)
    )
  ),
  created_at timestamptz NOT NULL DEFAULT now(),
  started_at timestamptz,
  completed_at timestamptz,
  UNIQUE (project_id, client_request_id)
);

ALTER TABLE workspace_operations
  DROP CONSTRAINT IF EXISTS workspace_operations_runner_job_fk;
ALTER TABLE workspace_operations
  ADD CONSTRAINT workspace_operations_runner_job_fk
  FOREIGN KEY (runner_job_id) REFERENCES runner_jobs(id) ON DELETE RESTRICT
  DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE workspace_snapshots
  DROP CONSTRAINT IF EXISTS workspace_snapshots_runner_job_fk;
ALTER TABLE workspace_snapshots
  ADD CONSTRAINT workspace_snapshots_runner_job_fk
  FOREIGN KEY (runner_job_id) REFERENCES runner_jobs(id) ON DELETE RESTRICT
  DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE IF NOT EXISTS runner_job_files (
  runner_job_id uuid NOT NULL REFERENCES runner_jobs(id) ON DELETE RESTRICT,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path text NOT NULL CHECK (
    path <> '' AND path NOT LIKE '/%' AND path NOT LIKE '%\\%'
    AND path !~ '(^|/)\.\.?(/|$)' AND path NOT LIKE '%//%'
    AND lower(path) !~ '(^|/)\.(git|fudian)(/|$)'
  ),
  sha256 text NOT NULL CHECK (sha256 ~ '^sha256:[0-9a-f]{64}$'),
  size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
  executable boolean NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (runner_job_id, path)
);

CREATE INDEX IF NOT EXISTS goal_workspaces_project_idx
  ON goal_workspaces (project_id, status, created_at);
CREATE INDEX IF NOT EXISTS workspace_snapshots_session_idx
  ON workspace_snapshots (session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS workspace_write_leases_session_idx
  ON workspace_write_leases (session_id, status, acquired_at DESC);
CREATE INDEX IF NOT EXISTS runner_jobs_session_idx
  ON runner_jobs (session_id, status, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS runner_job_files_casefold_idx
  ON runner_job_files (runner_job_id, lower(path));

ALTER TABLE goal_contributions
  ADD COLUMN IF NOT EXISTS runner_job_id uuid;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_contributions_runner_job_fk'
      AND conrelid = 'goal_contributions'::regclass
  ) THEN
    ALTER TABLE goal_contributions
      ADD CONSTRAINT goal_contributions_runner_job_fk
      FOREIGN KEY (runner_job_id) REFERENCES runner_jobs(id) ON DELETE RESTRICT;
  END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS goal_contributions_runner_job_idx
  ON goal_contributions (runner_job_id) WHERE runner_job_id IS NOT NULL;

CREATE OR REPLACE FUNCTION goal_validate_workspace_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  repository_project uuid;
BEGIN
  SELECT project_id INTO branch_project FROM goal_branches WHERE id = NEW.goal_branch_id;
  SELECT project_id INTO repository_project FROM project_git_repositories WHERE id = NEW.repository_id;
  IF branch_project IS DISTINCT FROM NEW.project_id
     OR repository_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'workspace belongs to another project aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_workspaces_scope
    BEFORE INSERT OR UPDATE OF project_id, goal_branch_id, repository_id
    ON goal_workspaces FOR EACH ROW EXECUTE FUNCTION goal_validate_workspace_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_workspace_child_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  workspace_project uuid;
  workspace_branch uuid;
  session_project uuid;
  session_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO workspace_project, workspace_branch
  FROM goal_workspaces WHERE id = NEW.workspace_id;
  IF workspace_project IS DISTINCT FROM NEW.project_id
     OR workspace_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'workspace child record belongs to another workspace aggregate'
      USING ERRCODE = '23514';
  END IF;
  IF to_jsonb(NEW) ? 'session_id' THEN
    EXECUTE 'SELECT project_id, goal_branch_id FROM goal_sessions WHERE id = $1'
      INTO session_project, session_branch USING NEW.session_id;
    IF session_project IS DISTINCT FROM NEW.project_id
       OR session_branch IS DISTINCT FROM NEW.goal_branch_id THEN
      RAISE EXCEPTION 'workspace child Session belongs to another aggregate'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$
DECLARE
  target_table text;
  trigger_name text;
BEGIN
  FOREACH target_table IN ARRAY ARRAY[
    'workspace_operations', 'workspace_snapshots', 'workspace_write_leases', 'runner_jobs'
  ]
  LOOP
    trigger_name := target_table || '_scope';
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = trigger_name AND tgrelid = target_table::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE INSERT OR UPDATE OF project_id, goal_branch_id, workspace_id '
        'ON %I FOR EACH ROW EXECUTE FUNCTION goal_validate_workspace_child_scope()',
        trigger_name, target_table
      );
    END IF;
  END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION goal_validate_workspace_policy_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  branch_proposal uuid;
  proposal_project uuid;
BEGIN
  SELECT project_id, creating_proposal_id INTO branch_project, branch_proposal
  FROM goal_branches WHERE id = NEW.goal_branch_id;
  SELECT project_id INTO proposal_project
  FROM goal_branch_proposals WHERE id = NEW.source_proposal_id;
  IF branch_project IS DISTINCT FROM NEW.project_id
     OR proposal_project IS DISTINCT FROM NEW.project_id
     OR branch_proposal IS DISTINCT FROM NEW.source_proposal_id THEN
    RAISE EXCEPTION 'workspace policy source does not match GoalBranch origin'
      USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM goal_branch_proposal_revisions
    WHERE proposal_id = NEW.source_proposal_id AND revision = NEW.source_proposal_revision
  ) THEN
    RAISE EXCEPTION 'workspace policy source revision does not exist'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_workspace_policies_scope
    BEFORE INSERT OR UPDATE OF project_id, goal_branch_id, source_proposal_id,
      source_proposal_revision
    ON goal_workspace_policies FOR EACH ROW EXECUTE FUNCTION goal_validate_workspace_policy_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_runner_file_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  job_project uuid;
BEGIN
  SELECT project_id INTO job_project FROM runner_jobs WHERE id = NEW.runner_job_id;
  IF job_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'Runner output file belongs to another project'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER runner_job_files_scope
    BEFORE INSERT OR UPDATE OF project_id, runner_job_id
    ON runner_job_files FOR EACH ROW EXECUTE FUNCTION goal_validate_runner_file_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_runner_contribution_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  job_project uuid;
  job_branch uuid;
  job_session uuid;
BEGIN
  IF NEW.runner_job_id IS NULL THEN
    RETURN NEW;
  END IF;
  SELECT project_id, goal_branch_id, session_id
    INTO job_project, job_branch, job_session
  FROM runner_jobs WHERE id = NEW.runner_job_id;
  IF job_project IS DISTINCT FROM NEW.project_id
     OR job_branch IS DISTINCT FROM NEW.goal_branch_id
     OR job_session IS DISTINCT FROM NEW.session_id THEN
    RAISE EXCEPTION 'Runner contribution belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_contributions_runner_job_scope
    BEFORE INSERT OR UPDATE OF project_id, goal_branch_id, session_id, runner_job_id
    ON goal_contributions FOR EACH ROW
    EXECUTE FUNCTION goal_validate_runner_contribution_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_workspace_lease_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.workspace_id IS DISTINCT FROM NEW.workspace_id
     OR OLD.client_request_id IS DISTINCT FROM NEW.client_request_id
     OR OLD.request_hash IS DISTINCT FROM NEW.request_hash
     OR OLD.fencing_token IS DISTINCT FROM NEW.fencing_token
     OR OLD.renewal_token_digest IS DISTINCT FROM NEW.renewal_token_digest
     OR OLD.base_commit IS DISTINCT FROM NEW.base_commit
     OR OLD.base_workspace_snapshot IS DISTINCT FROM NEW.base_workspace_snapshot
     OR OLD.allowed_writes IS DISTINCT FROM NEW.allowed_writes
     OR OLD.delete_paths IS DISTINCT FROM NEW.delete_paths
     OR OLD.capabilities IS DISTINCT FROM NEW.capabilities
     OR OLD.resource_policy IS DISTINCT FROM NEW.resource_policy
     OR OLD.output_key IS DISTINCT FROM NEW.output_key
     OR OLD.acquired_at IS DISTINCT FROM NEW.acquired_at
     OR OLD.hard_expires_at IS DISTINCT FROM NEW.hard_expires_at THEN
    RAISE EXCEPTION 'workspace Lease identity is immutable'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.status <> 'active' OR NEW.status NOT IN ('active', 'released', 'expired', 'cancelled', 'failed') THEN
    RAISE EXCEPTION 'invalid workspace Lease transition'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER workspace_write_leases_guard
    BEFORE UPDATE ON workspace_write_leases
    FOR EACH ROW EXECUTE FUNCTION goal_guard_workspace_lease_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_runner_job_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.workspace_id IS DISTINCT FROM NEW.workspace_id
     OR OLD.lease_id IS DISTINCT FROM NEW.lease_id
     OR OLD.client_request_id IS DISTINCT FROM NEW.client_request_id
     OR OLD.request_hash IS DISTINCT FROM NEW.request_hash
     OR OLD.spec IS DISTINCT FROM NEW.spec
     OR OLD.spec_hash IS DISTINCT FROM NEW.spec_hash
     OR OLD.runtime_digest IS DISTINCT FROM NEW.runtime_digest
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'RunnerJob request identity is immutable'
      USING ERRCODE = '55000';
  END IF;
  IF OLD.status IN ('succeeded', 'failed', 'timed_out', 'policy_denied', 'workspace_conflict', 'cancelled')
     AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal RunnerJob is immutable'
      USING ERRCODE = '55000';
  END IF;
  IF (OLD.status, NEW.status) NOT IN (
    ('prepared', 'running'), ('prepared', 'applying'), ('prepared', 'failed'),
    ('prepared', 'timed_out'), ('prepared', 'policy_denied'), ('prepared', 'cancelled'),
    ('running', 'applying'), ('running', 'failed'), ('running', 'timed_out'),
    ('running', 'policy_denied'), ('running', 'cancelled'),
    ('applying', 'applying'), ('applying', 'succeeded'),
    ('applying', 'workspace_conflict'), ('applying', 'failed')
  ) THEN
    RAISE EXCEPTION 'invalid RunnerJob transition'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER runner_jobs_guard
    BEFORE UPDATE ON runner_jobs
    FOR EACH ROW EXECUTE FUNCTION goal_guard_runner_job_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
DECLARE
  target_table text;
  trigger_name text;
BEGIN
  FOREACH target_table IN ARRAY ARRAY[
    'project_git_repositories', 'goal_workspace_policies', 'workspace_snapshots',
    'runner_job_files'
  ]
  LOOP
    trigger_name := target_table || '_immutable';
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = trigger_name AND tgrelid = target_table::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I '
        'FOR EACH ROW EXECUTE FUNCTION goal_reject_immutable_mutation()',
        trigger_name, target_table
      );
    END IF;
  END LOOP;
END;
$$;
