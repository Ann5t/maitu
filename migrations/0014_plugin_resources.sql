CREATE TABLE IF NOT EXISTS plugin_package_resources (
  id uuid PRIMARY KEY,
  plugin_package_id uuid NOT NULL REFERENCES plugin_packages(id) ON DELETE RESTRICT,
  path text NOT NULL CHECK (
    path <> ''
    AND char_length(path) <= 1000
    AND position(':' IN path) = 0
    AND position(E'\\' IN path) = 0
    AND path !~ '(^/|//|(^|/)\.{1,2}(/|$))'
  ),
  media_type text NOT NULL CHECK (
    media_type <> '' AND char_length(media_type) <= 160 AND position('/' IN media_type) > 0
  ),
  content_digest text NOT NULL CHECK (content_digest ~ '^sha256:[0-9a-f]{64}$'),
  content bytea NOT NULL CHECK (octet_length(content) <= 524288),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (plugin_package_id, path)
);

CREATE INDEX IF NOT EXISTS plugin_package_resources_package_idx
  ON plugin_package_resources (plugin_package_id, path);

DO $$ BEGIN
  CREATE TRIGGER plugin_package_resources_immutable
    BEFORE UPDATE OR DELETE ON plugin_package_resources FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS plugin_resource_reads (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  environment_fingerprint text NOT NULL CHECK (
    environment_fingerprint ~ '^sha256:[0-9a-f]{64}$'
  ),
  requested_resources jsonb NOT NULL CHECK (jsonb_typeof(requested_resources) = 'array'),
  served_resources jsonb NOT NULL CHECK (jsonb_typeof(served_resources) = 'array'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, client_request_id)
);

CREATE INDEX IF NOT EXISTS plugin_resource_reads_session_idx
  ON plugin_resource_reads (session_id, created_at DESC);

DO $$ BEGIN
  CREATE TRIGGER plugin_resource_reads_immutable
    BEFORE UPDATE OR DELETE ON plugin_resource_reads FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_plugin_resource_read_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  observed_project uuid;
BEGIN
  SELECT project_id INTO observed_project FROM goal_sessions WHERE id = NEW.session_id;
  IF observed_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'plugin resource read belongs to another Project/Session aggregate'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER plugin_resource_reads_scope
    BEFORE INSERT ON plugin_resource_reads FOR EACH ROW
    EXECUTE FUNCTION goal_validate_plugin_resource_read_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
