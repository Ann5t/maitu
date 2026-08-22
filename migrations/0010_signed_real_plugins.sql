CREATE TABLE IF NOT EXISTS plugin_publishers (
  publisher_id text PRIMARY KEY CHECK (
    publisher_id ~ '^[a-z][a-z0-9._-]{0,159}$'
  ),
  display_name text NOT NULL CHECK (display_name <> ''),
  public_key text NOT NULL UNIQUE CHECK (public_key ~ '^[0-9a-f]{64}$'),
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
  created_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz,
  revocation_reason text,
  CHECK (
    (status = 'active' AND revoked_at IS NULL AND revocation_reason IS NULL)
    OR (status = 'revoked' AND revoked_at IS NOT NULL AND revocation_reason <> '')
  )
);

CREATE TABLE IF NOT EXISTS plugin_installations (
  id uuid PRIMARY KEY,
  plugin_package_id uuid NOT NULL UNIQUE REFERENCES plugin_packages(id) ON DELETE RESTRICT,
  publisher_id text NOT NULL REFERENCES plugin_publishers(publisher_id) ON DELETE RESTRICT,
  signature text NOT NULL CHECK (signature ~ '^[0-9a-f]{128}$'),
  statement jsonb NOT NULL CHECK (jsonb_typeof(statement) = 'object'),
  statement_digest text NOT NULL CHECK (statement_digest ~ '^sha256:[0-9a-f]{64}$'),
  runtime_image_digest text NOT NULL CHECK (runtime_image_digest ~ '^sha256:[0-9a-f]{64}$'),
  runtime_entry_digest text NOT NULL CHECK (runtime_entry_digest ~ '^sha256:[0-9a-f]{64}$'),
  runner_digest text NOT NULL CHECK (runner_digest ~ '^sha256:[0-9a-f]{64}$'),
  self_test jsonb NOT NULL CHECK (jsonb_typeof(self_test) = 'object'),
  self_test_digest text NOT NULL CHECK (self_test_digest ~ '^sha256:[0-9a-f]{64}$'),
  status text NOT NULL DEFAULT 'installed' CHECK (status IN ('installed', 'revoked')),
  installed_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz,
  revocation_reason text,
  CHECK (
    (status = 'installed' AND revoked_at IS NULL AND revocation_reason IS NULL)
    OR (status = 'revoked' AND revoked_at IS NOT NULL AND revocation_reason <> '')
  )
);

CREATE TABLE IF NOT EXISTS plugin_install_requests (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  plugin_id text NOT NULL CHECK (plugin_id ~ '^[a-z][a-z0-9._-]{1,159}$'),
  version_requirement text NOT NULL CHECK (version_requirement <> ''),
  capability text NOT NULL CHECK (capability <> ''),
  reason text NOT NULL CHECK (reason <> ''),
  status text NOT NULL DEFAULT 'requested' CHECK (status IN ('requested', 'available', 'rejected')),
  created_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  UNIQUE (project_id, client_request_id)
);

CREATE TABLE IF NOT EXISTS tool_execution_requests (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  runner_job_id uuid NOT NULL UNIQUE REFERENCES runner_jobs(id) ON DELETE RESTRICT,
  plugin_installation_id uuid NOT NULL REFERENCES plugin_installations(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  plugin_id text NOT NULL,
  plugin_version text NOT NULL,
  plugin_digest text NOT NULL CHECK (plugin_digest ~ '^sha256:[0-9a-f]{64}$'),
  runtime_image_digest text NOT NULL CHECK (runtime_image_digest ~ '^sha256:[0-9a-f]{64}$'),
  runtime_entry_digest text NOT NULL CHECK (runtime_entry_digest ~ '^sha256:[0-9a-f]{64}$'),
  tool_name text NOT NULL,
  input jsonb NOT NULL CHECK (jsonb_typeof(input) = 'object'),
  environment_fingerprint text NOT NULL CHECK (environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'),
  base_workspace_snapshot text NOT NULL CHECK (base_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, client_request_id)
);

ALTER TABLE tool_calls
  ADD COLUMN IF NOT EXISTS runner_job_id uuid;
DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'tool_calls_runner_job_fk'
      AND conrelid = 'tool_calls'::regclass
  ) THEN
    ALTER TABLE tool_calls
      ADD CONSTRAINT tool_calls_runner_job_fk
      FOREIGN KEY (runner_job_id) REFERENCES runner_jobs(id) ON DELETE RESTRICT;
  END IF;
END $$;
CREATE UNIQUE INDEX IF NOT EXISTS tool_calls_runner_job_idx
  ON tool_calls (runner_job_id) WHERE runner_job_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS plugin_installations_publisher_idx
  ON plugin_installations (publisher_id, status, installed_at DESC);
CREATE INDEX IF NOT EXISTS plugin_install_requests_session_idx
  ON plugin_install_requests (session_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS tool_execution_requests_session_idx
  ON tool_execution_requests (session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS tool_execution_requests_installation_idx
  ON tool_execution_requests (plugin_installation_id, created_at DESC);

CREATE OR REPLACE FUNCTION goal_guard_plugin_publisher_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.publisher_id IS DISTINCT FROM NEW.publisher_id
     OR OLD.display_name IS DISTINCT FROM NEW.display_name
     OR OLD.public_key IS DISTINCT FROM NEW.public_key
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'plugin publisher identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'revoked'
     OR NOT (OLD.status = 'active' AND NEW.status IN ('active', 'revoked')) THEN
    RAISE EXCEPTION 'invalid plugin publisher transition' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER plugin_publishers_guard
    BEFORE UPDATE ON plugin_publishers FOR EACH ROW
    EXECUTE FUNCTION goal_guard_plugin_publisher_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TRIGGER plugin_publishers_immutable_delete
    BEFORE DELETE ON plugin_publishers FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_plugin_installation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.plugin_package_id IS DISTINCT FROM NEW.plugin_package_id
     OR OLD.publisher_id IS DISTINCT FROM NEW.publisher_id
     OR OLD.signature IS DISTINCT FROM NEW.signature
     OR OLD.statement IS DISTINCT FROM NEW.statement
     OR OLD.statement_digest IS DISTINCT FROM NEW.statement_digest
     OR OLD.runtime_image_digest IS DISTINCT FROM NEW.runtime_image_digest
     OR OLD.runtime_entry_digest IS DISTINCT FROM NEW.runtime_entry_digest
     OR OLD.runner_digest IS DISTINCT FROM NEW.runner_digest
     OR OLD.self_test IS DISTINCT FROM NEW.self_test
     OR OLD.self_test_digest IS DISTINCT FROM NEW.self_test_digest
     OR OLD.installed_at IS DISTINCT FROM NEW.installed_at THEN
    RAISE EXCEPTION 'plugin installation proof is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'revoked'
     OR NOT (OLD.status = 'installed' AND NEW.status IN ('installed', 'revoked')) THEN
    RAISE EXCEPTION 'invalid plugin installation transition' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER plugin_installations_guard
    BEFORE UPDATE ON plugin_installations FOR EACH ROW
    EXECUTE FUNCTION goal_guard_plugin_installation_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TRIGGER plugin_installations_immutable_delete
    BEFORE DELETE ON plugin_installations FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
DECLARE
  target_table text;
BEGIN
  FOREACH target_table IN ARRAY ARRAY['plugin_install_requests', 'tool_execution_requests']
  LOOP
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = target_table || '_immutable' AND tgrelid = target_table::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I FOR EACH ROW '
        'EXECUTE FUNCTION goal_reject_immutable_mutation()',
        target_table || '_immutable', target_table
      );
    END IF;
  END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION goal_validate_plugin_session_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  observed_project uuid;
  observed_branch uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO observed_project, observed_branch
  FROM goal_sessions WHERE id = NEW.session_id;
  IF observed_project IS DISTINCT FROM NEW.project_id
     OR observed_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'plugin record belongs to another Session aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$
DECLARE
  target_table text;
BEGIN
  FOREACH target_table IN ARRAY ARRAY['plugin_install_requests', 'tool_execution_requests']
  LOOP
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = target_table || '_scope' AND tgrelid = target_table::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE INSERT ON %I FOR EACH ROW '
        'EXECUTE FUNCTION goal_validate_plugin_session_scope()',
        target_table || '_scope', target_table
      );
    END IF;
  END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION goal_validate_tool_execution_runner_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  runner_project uuid;
  runner_branch uuid;
  runner_session uuid;
BEGIN
  SELECT project_id, goal_branch_id, session_id
  INTO runner_project, runner_branch, runner_session
  FROM runner_jobs WHERE id = NEW.runner_job_id;
  IF runner_project IS DISTINCT FROM NEW.project_id
     OR runner_branch IS DISTINCT FROM NEW.goal_branch_id
     OR runner_session IS DISTINCT FROM NEW.session_id THEN
    RAISE EXCEPTION 'ToolExecution runner job belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER tool_execution_requests_runner_scope
    BEFORE INSERT ON tool_execution_requests FOR EACH ROW
    EXECUTE FUNCTION goal_validate_tool_execution_runner_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_tool_call_runner_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  runner_project uuid;
  runner_branch uuid;
  runner_session uuid;
BEGIN
  IF NEW.runner_job_id IS NULL THEN
    RETURN NEW;
  END IF;
  SELECT project_id, goal_branch_id, session_id
  INTO runner_project, runner_branch, runner_session
  FROM runner_jobs WHERE id = NEW.runner_job_id;
  IF runner_project IS DISTINCT FROM NEW.project_id
     OR runner_branch IS DISTINCT FROM NEW.goal_branch_id
     OR runner_session IS DISTINCT FROM NEW.session_id THEN
    RAISE EXCEPTION 'ToolCall runner job belongs to another aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER tool_calls_runner_scope
    BEFORE INSERT ON tool_calls FOR EACH ROW
    EXECUTE FUNCTION goal_validate_tool_call_runner_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
