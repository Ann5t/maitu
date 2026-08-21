CREATE TABLE IF NOT EXISTS input_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  status text NOT NULL DEFAULT 'staging' CHECK (status IN (
    'staging', 'verified', 'available', 'imported', 'rejected', 'quarantined'
  )),
  original_filename text NOT NULL,
  display_name text NOT NULL,
  declared_media_type text,
  trusted_media_type text,
  declared_size bigint NOT NULL CHECK (declared_size >= 0),
  actual_size bigint NOT NULL DEFAULT 0 CHECK (actual_size >= 0),
  sha256 text CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$'),
  storage_key text NOT NULL,
  verification jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(verification) = 'object'),
  import_mode text CHECK (import_mode IS NULL OR import_mode IN (
    'worktree_copy', 'artifact_reference', 'read_only_mount'
  )),
  inbox_relative_path text,
  artifact_id uuid REFERENCES artifacts(id) ON DELETE SET NULL,
  finish_client_request_id uuid,
  finish_request_hash text CHECK (
    finish_request_hash IS NULL OR finish_request_hash ~ '^sha256:[0-9a-f]{64}$'
  ),
  import_client_request_id uuid,
  import_request_hash text CHECK (
    import_request_hash IS NULL OR import_request_hash ~ '^sha256:[0-9a-f]{64}$'
  ),
  created_at timestamptz NOT NULL DEFAULT now(),
  verified_at timestamptz,
  available_at timestamptz,
  imported_at timestamptz,
  UNIQUE (project_id, client_request_id),
  CHECK (actual_size <= declared_size)
);

CREATE TABLE IF NOT EXISTS input_artifact_chunks (
  input_artifact_id uuid NOT NULL REFERENCES input_artifacts(id) ON DELETE CASCADE,
  offset_bytes bigint NOT NULL CHECK (offset_bytes >= 0),
  size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
  sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
  storage_key text NOT NULL UNIQUE,
  client_request_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (input_artifact_id, offset_bytes),
  UNIQUE (input_artifact_id, client_request_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS input_artifacts_finish_request_idx
  ON input_artifacts (project_id, finish_client_request_id)
  WHERE finish_client_request_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS input_artifacts_import_request_idx
  ON input_artifacts (project_id, import_client_request_id)
  WHERE import_client_request_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS input_artifacts_session_status_idx
  ON input_artifacts (session_id, status, created_at DESC);

DO $$ BEGIN
  CREATE TRIGGER input_artifact_chunks_immutable
    BEFORE UPDATE OR DELETE ON input_artifact_chunks
    FOR EACH ROW EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
