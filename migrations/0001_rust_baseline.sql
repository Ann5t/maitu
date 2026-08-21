CREATE TABLE IF NOT EXISTS projects (
  id uuid PRIMARY KEY,
  title text NOT NULL,
  intent text NOT NULL,
  state text NOT NULL DEFAULT 'shaping' CHECK (state IN (
    'shaping', 'active', 'waiting', 'paused', 'completed', 'stopped', 'archived'
  )),
  current_focus text,
  completion_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS project_branches (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name text NOT NULL,
  purpose text NOT NULL DEFAULT '',
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'waiting', 'integrated', 'closed')),
  is_main integer NOT NULL DEFAULT 0 CHECK (is_main IN (0, 1)),
  color text NOT NULL DEFAULT '#6f7cff',
  forked_from_node_id uuid,
  head_node_id uuid,
  client_request_id uuid UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  closed_at timestamptz
);

CREATE TABLE IF NOT EXISTS project_nodes (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  branch_id uuid NOT NULL REFERENCES project_branches(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('origin', 'work', 'result', 'decision', 'merge')),
  title text NOT NULL,
  summary text NOT NULL,
  outcome text NOT NULL DEFAULT 'open' CHECK (outcome IN (
    'open', 'useful', 'refuted', 'mixed', 'blocked', 'inconclusive'
  )),
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'system')),
  client_request_id uuid UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now(),
  resolved_at timestamptz
);

DO $$ BEGIN
  ALTER TABLE project_branches
    ADD CONSTRAINT project_branches_forked_from_fk
    FOREIGN KEY (forked_from_node_id) REFERENCES project_nodes(id) ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE project_branches
    ADD CONSTRAINT project_branches_head_fk
    FOREIGN KEY (head_node_id) REFERENCES project_nodes(id) ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS project_node_edges (
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  parent_node_id uuid NOT NULL REFERENCES project_nodes(id) ON DELETE CASCADE,
  child_node_id uuid NOT NULL REFERENCES project_nodes(id) ON DELETE CASCADE,
  relation text NOT NULL CHECK (relation IN ('continue', 'fork', 'merge')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (parent_node_id, child_node_id)
);

CREATE TABLE IF NOT EXISTS project_contributions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  node_id uuid NOT NULL REFERENCES project_nodes(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN (
    'artifact', 'finding', 'evidence', 'decision', 'condition', 'other'
  )),
  title text NOT NULL,
  body text NOT NULL,
  reference_uri text,
  scope text,
  reopen_when text,
  status text NOT NULL DEFAULT 'candidate' CHECK (status IN ('candidate', 'accepted', 'superseded')),
  accepted_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS branch_merges (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  source_branch_id uuid NOT NULL REFERENCES project_branches(id) ON DELETE RESTRICT,
  target_branch_id uuid NOT NULL REFERENCES project_branches(id) ON DELETE RESTRICT,
  result_node_id uuid NOT NULL UNIQUE REFERENCES project_nodes(id) ON DELETE RESTRICT,
  summary text NOT NULL,
  client_request_id uuid NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now(),
  accepted_contribution_ids jsonb NOT NULL DEFAULT '[]'::jsonb
);

CREATE TABLE IF NOT EXISTS outcome_contracts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
  desired_outcome text NOT NULL,
  success_evidence jsonb NOT NULL DEFAULT '[]'::jsonb,
  constraints jsonb NOT NULL DEFAULT '[]'::jsonb,
  non_goals jsonb NOT NULL DEFAULT '[]'::jsonb,
  confirmation_question text NOT NULL,
  contradictions jsonb NOT NULL DEFAULT '[]'::jsonb,
  status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'confirmed', 'superseded')),
  version integer NOT NULL DEFAULT 1 CHECK (version > 0),
  confirmed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS action_runs (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  kind text NOT NULL,
  title text NOT NULL,
  owner text NOT NULL CHECK (owner IN ('human', 'agent')),
  status text NOT NULL CHECK (status IN (
    'proposed', 'ready', 'running', 'blocked', 'completed', 'failed', 'cancelled'
  )),
  expected_signal text,
  output_summary text,
  requires_approval integer NOT NULL DEFAULT 0 CHECK (requires_approval IN (0, 1)),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  node_id uuid REFERENCES project_nodes(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  action_run_id uuid REFERENCES action_runs(id) ON DELETE SET NULL,
  title text NOT NULL,
  kind text NOT NULL,
  storage_path text NOT NULL,
  media_type text NOT NULL,
  sha256 text NOT NULL,
  version integer NOT NULL DEFAULT 1 CHECK (version > 0),
  status text NOT NULL DEFAULT 'draft' CHECK (status IN (
    'draft', 'review', 'approved', 'released', 'superseded'
  )),
  created_at timestamptz NOT NULL DEFAULT now(),
  approved_at timestamptz,
  node_id uuid REFERENCES project_nodes(id) ON DELETE SET NULL,
  UNIQUE (project_id, storage_path, version)
);

CREATE TABLE IF NOT EXISTS quality_gates (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title text NOT NULL,
  criteria jsonb NOT NULL DEFAULT '[]'::jsonb,
  status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'passed', 'failed', 'waived')),
  required integer NOT NULL DEFAULT 1 CHECK (required IN (0, 1)),
  created_at timestamptz NOT NULL DEFAULT now(),
  resolved_at timestamptz,
  node_id uuid REFERENCES project_nodes(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS evidence (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  action_run_id uuid REFERENCES action_runs(id) ON DELETE SET NULL,
  artifact_id uuid REFERENCES artifacts(id) ON DELETE SET NULL,
  kind text NOT NULL,
  summary text NOT NULL,
  source_uri text,
  stance text NOT NULL DEFAULT 'observes' CHECK (stance IN (
    'supports', 'refutes', 'blocks', 'unblocks', 'observes'
  )),
  confidence integer CHECK (confidence IS NULL OR confidence BETWEEN 0 AND 100),
  observed_at timestamptz NOT NULL DEFAULT now(),
  node_id uuid REFERENCES project_nodes(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS decision_checkpoints (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  decision text NOT NULL CHECK (decision IN (
    'continue', 'adjust', 'blocked', 'evidence_gap', 'falsified', 'reopen',
    'release', 'pause', 'complete'
  )),
  rationale text NOT NULL,
  confidence integer CHECK (confidence IS NULL OR confidence BETWEEN 0 AND 100),
  retry_when text,
  supersedes_id uuid REFERENCES decision_checkpoints(id) ON DELETE SET NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  node_id uuid REFERENCES project_nodes(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS project_events (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  event_type text NOT NULL,
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'system')),
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS projects_updated_at_idx ON projects (updated_at DESC);
CREATE INDEX IF NOT EXISTS project_branches_project_idx ON project_branches (project_id, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS project_branches_one_main_idx ON project_branches (project_id) WHERE is_main = 1;
CREATE INDEX IF NOT EXISTS project_nodes_project_idx ON project_nodes (project_id, created_at);
CREATE INDEX IF NOT EXISTS project_nodes_branch_idx ON project_nodes (branch_id, created_at);
CREATE INDEX IF NOT EXISTS project_node_edges_project_idx ON project_node_edges (project_id, created_at);
CREATE INDEX IF NOT EXISTS project_contributions_node_idx ON project_contributions (node_id, created_at);
CREATE INDEX IF NOT EXISTS project_contributions_project_status_idx ON project_contributions (project_id, status);
CREATE INDEX IF NOT EXISTS action_runs_project_status_idx ON action_runs (project_id, status);
CREATE INDEX IF NOT EXISTS quality_gates_project_status_idx ON quality_gates (project_id, status);
CREATE INDEX IF NOT EXISTS evidence_project_idx ON evidence (project_id, observed_at DESC);
CREATE INDEX IF NOT EXISTS decision_checkpoints_project_idx ON decision_checkpoints (project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS project_events_project_idx ON project_events (project_id, created_at DESC);

