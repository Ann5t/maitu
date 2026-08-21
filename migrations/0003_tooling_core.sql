CREATE TABLE IF NOT EXISTS plugin_packages (
  id uuid PRIMARY KEY,
  plugin_id text NOT NULL,
  version text NOT NULL,
  content_digest text NOT NULL CHECK (content_digest ~ '^sha256:[0-9a-f]{64}$'),
  manifest jsonb NOT NULL CHECK (jsonb_typeof(manifest) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (plugin_id, version, content_digest)
);

CREATE TABLE IF NOT EXISTS environment_manifests (
  id uuid PRIMARY KEY,
  fingerprint text NOT NULL UNIQUE CHECK (fingerprint ~ '^sha256:[0-9a-f]{64}$'),
  manifest jsonb NOT NULL CHECK (jsonb_typeof(manifest) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS session_environment_bindings (
  session_id uuid PRIMARY KEY REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  environment_manifest_id uuid NOT NULL REFERENCES environment_manifests(id) ON DELETE RESTRICT,
  environment_fingerprint text NOT NULL CHECK (
    environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  inherited_from_session_id uuid REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  bound_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS tool_calls (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  plugin_id text NOT NULL,
  plugin_version text NOT NULL,
  plugin_digest text NOT NULL CHECK (plugin_digest ~ '^sha256:[0-9a-f]{64}$'),
  tool_name text NOT NULL,
  input jsonb NOT NULL,
  environment_fingerprint text NOT NULL CHECK (
    environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  base_workspace_snapshot text NOT NULL CHECK (
    base_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  allowed_writes jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(allowed_writes) = 'array'),
  timeout_seconds integer NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 3600),
  status text NOT NULL CHECK (status IN (
    'succeeded', 'failed', 'timed_out', 'cancelled', 'workspace_conflict', 'policy_denied'
  )),
  result jsonb NOT NULL CHECK (jsonb_typeof(result) = 'object'),
  started_at timestamptz NOT NULL,
  completed_at timestamptz NOT NULL,
  UNIQUE (project_id, client_request_id)
);

CREATE TABLE IF NOT EXISTS tool_leases (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  plugin_id text NOT NULL,
  plugin_version text NOT NULL,
  plugin_digest text NOT NULL CHECK (plugin_digest ~ '^sha256:[0-9a-f]{64}$'),
  tool_name text NOT NULL,
  environment_fingerprint text NOT NULL CHECK (
    environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  base_workspace_snapshot text NOT NULL CHECK (
    base_workspace_snapshot ~ '^sha256:[0-9a-f]{64}$'
  ),
  status text NOT NULL CHECK (status IN (
    'requested', 'active', 'expired', 'released', 'failed', 'cancelled'
  )),
  renewal_token_digest text NOT NULL CHECK (
    renewal_token_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  resource_policy jsonb NOT NULL CHECK (jsonb_typeof(resource_policy) = 'object'),
  endpoint_refs jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(endpoint_refs) = 'array'),
  retained_outputs jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(retained_outputs) = 'array'),
  created_at timestamptz NOT NULL DEFAULT now(),
  last_heartbeat_at timestamptz,
  soft_expires_at timestamptz NOT NULL,
  hard_expires_at timestamptz NOT NULL,
  completed_at timestamptz,
  CHECK (soft_expires_at <= hard_expires_at)
);

CREATE INDEX IF NOT EXISTS plugin_packages_catalog_idx
  ON plugin_packages (plugin_id, version, created_at);
CREATE INDEX IF NOT EXISTS session_environment_bindings_project_idx
  ON session_environment_bindings (project_id, goal_branch_id);
CREATE INDEX IF NOT EXISTS tool_calls_session_idx
  ON tool_calls (session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS tool_leases_session_status_idx
  ON tool_leases (session_id, status, created_at DESC);

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'plugin_packages',
    'environment_manifests',
    'session_environment_bindings',
    'tool_calls'
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
