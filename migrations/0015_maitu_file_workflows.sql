CREATE TABLE IF NOT EXISTS maitu_sources (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  filename text NOT NULL,
  content text NOT NULL,
  sha256 text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, project_id)
);

CREATE TABLE IF NOT EXISTS maitu_tasks (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title text NOT NULL,
  instruction text NOT NULL,
  output_filename text NOT NULL,
  source_ids jsonb NOT NULL DEFAULT '[]',
  status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','queued','running','produced','failed','interrupted')),
  wait_reason text,
  latest_attempt_id uuid,
  accepted_attempt_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, project_id)
);

CREATE TABLE IF NOT EXISTS maitu_task_dependencies (
  project_id uuid NOT NULL,
  task_id uuid NOT NULL,
  parent_task_id uuid NOT NULL,
  PRIMARY KEY (task_id, parent_task_id),
  FOREIGN KEY (task_id, project_id) REFERENCES maitu_tasks(id, project_id) ON DELETE CASCADE,
  FOREIGN KEY (parent_task_id, project_id) REFERENCES maitu_tasks(id, project_id) ON DELETE CASCADE,
  CHECK (task_id <> parent_task_id)
);

CREATE TABLE IF NOT EXISTS maitu_attempts (
  id uuid PRIMARY KEY,
  task_id uuid NOT NULL REFERENCES maitu_tasks(id) ON DELETE CASCADE,
  number integer NOT NULL CHECK (number > 0),
  status text NOT NULL CHECK (status IN ('queued','running','produced','failed','interrupted')),
  input_snapshot jsonb,
  provider_base_url text,
  model text,
  artifact_id uuid REFERENCES artifacts(id),
  error_code text,
  error_message text,
  usage jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  started_at timestamptz,
  request_started_at timestamptz,
  response_received_at timestamptz,
  completed_at timestamptz,
  UNIQUE (task_id, number),
  UNIQUE (id, task_id)
);

DO $$ BEGIN
  ALTER TABLE maitu_tasks ADD CONSTRAINT maitu_task_latest_attempt_fk
    FOREIGN KEY (latest_attempt_id, id) REFERENCES maitu_attempts(id, task_id);
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
DO $$ BEGIN
  ALTER TABLE maitu_tasks ADD CONSTRAINT maitu_task_accepted_attempt_fk
    FOREIGN KEY (accepted_attempt_id, id) REFERENCES maitu_attempts(id, task_id);
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS maitu_attempt_dependencies (
  attempt_id uuid NOT NULL REFERENCES maitu_attempts(id) ON DELETE CASCADE,
  parent_task_id uuid NOT NULL REFERENCES maitu_tasks(id),
  source_attempt_id uuid,
  PRIMARY KEY (attempt_id, parent_task_id),
  FOREIGN KEY (source_attempt_id, parent_task_id) REFERENCES maitu_attempts(id, task_id)
);

CREATE TABLE IF NOT EXISTS maitu_attempt_events (
  id bigserial PRIMARY KEY,
  attempt_id uuid NOT NULL REFERENCES maitu_attempts(id) ON DELETE CASCADE,
  phase text NOT NULL,
  message text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS maitu_tasks_queue ON maitu_tasks(created_at) WHERE status = 'queued';
CREATE INDEX IF NOT EXISTS maitu_tasks_project ON maitu_tasks(project_id, created_at);
CREATE INDEX IF NOT EXISTS maitu_attempts_task ON maitu_attempts(task_id, number DESC);
CREATE INDEX IF NOT EXISTS maitu_events_attempt ON maitu_attempt_events(attempt_id, id);
