ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS task_kind text NOT NULL DEFAULT 'file'
  CHECK (task_kind IN ('file', 'plan', 'code'));
ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS acceptance_criteria text NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS maitu_plans (
  attempt_id uuid PRIMARY KEY REFERENCES maitu_attempts(id),
  project_id uuid NOT NULL REFERENCES projects(id),
  proposal jsonb NOT NULL,
  adopted_proposal jsonb,
  adopted_task_ids jsonb,
  adopted_at timestamptz
);

CREATE TABLE IF NOT EXISTS maitu_code_projects (
  project_id uuid PRIMARY KEY REFERENCES projects(id),
  import_request_id uuid NOT NULL UNIQUE,
  source_name text NOT NULL,
  import_hash text NOT NULL,
  initial_commit text NOT NULL,
  accepted_commit text NOT NULL,
  checks jsonb NOT NULL,
  file_count integer NOT NULL,
  size_bytes bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS maitu_code_attempts (
  attempt_id uuid PRIMARY KEY REFERENCES maitu_attempts(id),
  project_id uuid NOT NULL REFERENCES maitu_code_projects(project_id),
  base_commit text NOT NULL,
  workspace_key text NOT NULL UNIQUE,
  candidate_commit text,
  patch_artifact_id uuid REFERENCES artifacts(id),
  check_results jsonb NOT NULL DEFAULT '[]',
  adopted_commit text,
  adopted_at timestamptz
);

CREATE TABLE IF NOT EXISTS maitu_execution_operations (
  id bigserial PRIMARY KEY,
  attempt_id uuid NOT NULL REFERENCES maitu_attempts(id),
  kind text NOT NULL CHECK (kind IN ('model','tool','check','integration')),
  label text NOT NULL,
  status text NOT NULL CHECK (status IN ('running','succeeded','failed','interrupted')),
  input jsonb NOT NULL,
  output jsonb,
  started_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz
);

CREATE INDEX IF NOT EXISTS maitu_operations_attempt ON maitu_execution_operations(attempt_id,id);
