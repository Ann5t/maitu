CREATE TABLE IF NOT EXISTS app_users (
  id smallint PRIMARY KEY DEFAULT 1 CHECK (id = 1),
  username text NOT NULL UNIQUE CHECK (
    username ~ '^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$'
  ),
  password_hash text NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
  password_version bigint NOT NULL DEFAULT 1 CHECK (password_version > 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  password_changed_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS auth_recovery_codes (
  id uuid PRIMARY KEY,
  user_id smallint NOT NULL REFERENCES app_users(id) ON DELETE RESTRICT,
  code_digest text NOT NULL UNIQUE CHECK (code_digest ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  used_at timestamptz,
  used_request_id uuid UNIQUE
);

CREATE INDEX IF NOT EXISTS auth_recovery_codes_open_idx
  ON auth_recovery_codes (user_id, created_at) WHERE used_at IS NULL;

CREATE TABLE IF NOT EXISTS auth_sessions (
  id uuid PRIMARY KEY,
  user_id smallint NOT NULL REFERENCES app_users(id) ON DELETE RESTRICT,
  token_digest text NOT NULL UNIQUE CHECK (token_digest ~ '^sha256:[0-9a-f]{64}$'),
  csrf_digest text NOT NULL CHECK (csrf_digest ~ '^sha256:[0-9a-f]{64}$'),
  password_version bigint NOT NULL CHECK (password_version > 0),
  client_ip_digest text NOT NULL CHECK (client_ip_digest ~ '^sha256:[0-9a-f]{64}$'),
  user_agent_digest text NOT NULL CHECK (user_agent_digest ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  rotated_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  revocation_reason text,
  replaced_by_session_id uuid REFERENCES auth_sessions(id) ON DELETE RESTRICT,
  CHECK (expires_at > created_at),
  CHECK (
    (revoked_at IS NULL AND revocation_reason IS NULL AND replaced_by_session_id IS NULL)
    OR (revoked_at IS NOT NULL AND revocation_reason <> '')
  )
);

CREATE INDEX IF NOT EXISTS auth_sessions_active_idx
  ON auth_sessions (token_digest, expires_at) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS auth_sessions_user_idx
  ON auth_sessions (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS auth_rate_limit_buckets (
  scope text NOT NULL CHECK (scope IN ('login_ip', 'login_account', 'recovery_ip',
    'recovery_account', 'mutation_session', 'high_cost_session')),
  key_digest text NOT NULL CHECK (key_digest ~ '^sha256:[0-9a-f]{64}$'),
  window_started_at timestamptz NOT NULL,
  attempt_count integer NOT NULL CHECK (attempt_count >= 0),
  blocked_until timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (scope, key_digest)
);

CREATE INDEX IF NOT EXISTS auth_rate_limit_expiry_idx
  ON auth_rate_limit_buckets (updated_at, blocked_until);

CREATE TABLE IF NOT EXISTS security_audit_events (
  id uuid PRIMARY KEY,
  sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  event_type text NOT NULL CHECK (event_type <> ''),
  outcome text NOT NULL CHECK (outcome IN ('allowed', 'denied', 'failed')),
  user_id smallint REFERENCES app_users(id) ON DELETE RESTRICT,
  session_id uuid REFERENCES auth_sessions(id) ON DELETE RESTRICT,
  request_id text,
  client_ip_digest text NOT NULL CHECK (client_ip_digest ~ '^sha256:[0-9a-f]{64}$'),
  user_agent_digest text NOT NULL CHECK (user_agent_digest ~ '^sha256:[0-9a-f]{64}$'),
  detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS security_audit_events_created_idx
  ON security_audit_events (created_at DESC, sequence DESC);

CREATE TABLE IF NOT EXISTS storage_reconciliation_runs (
  id uuid PRIMARY KEY,
  mode text NOT NULL CHECK (mode IN ('scan', 'quarantine', 'restore')),
  status text NOT NULL CHECK (status IN ('running', 'completed', 'failed')),
  retention_cutoff timestamptz NOT NULL,
  roots_digest text NOT NULL CHECK (roots_digest ~ '^sha256:[0-9a-f]{64}$'),
  summary jsonb CHECK (summary IS NULL OR jsonb_typeof(summary) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  CHECK (
    (status = 'running' AND completed_at IS NULL)
    OR (status <> 'running' AND completed_at IS NOT NULL)
  )
);

CREATE TABLE IF NOT EXISTS storage_reconciliation_items (
  id uuid PRIMARY KEY,
  run_id uuid NOT NULL REFERENCES storage_reconciliation_runs(id) ON DELETE RESTRICT,
  storage_class text NOT NULL CHECK (storage_class IN (
    'artifact', 'input_chunk', 'input_object', 'repository', 'worktree', 'runner_output'
  )),
  relative_path text NOT NULL CHECK (
    relative_path <> '' AND relative_path NOT LIKE '/%' AND relative_path NOT LIKE '%\\%'
    AND relative_path NOT LIKE '%//%' AND relative_path !~ '(^|/)\.\.?(/|$)'
  ),
  state text NOT NULL CHECK (state IN ('referenced', 'orphan', 'missing', 'quarantined', 'restored')),
  content_digest text CHECK (content_digest IS NULL OR content_digest ~ '^sha256:[0-9a-f]{64}$'),
  size_bytes bigint CHECK (size_bytes IS NULL OR size_bytes >= 0),
  observed_mtime timestamptz,
  eligible_after timestamptz,
  quarantine_path text,
  detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (run_id, storage_class, relative_path),
  CHECK (
    (state IN ('quarantined', 'restored') AND quarantine_path IS NOT NULL)
    OR (state NOT IN ('quarantined', 'restored') AND quarantine_path IS NULL)
  )
);

CREATE INDEX IF NOT EXISTS storage_reconciliation_items_state_idx
  ON storage_reconciliation_items (run_id, state, storage_class);

CREATE OR REPLACE FUNCTION guard_auth_recovery_code()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'recovery code audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.user_id IS DISTINCT FROM NEW.user_id
     OR OLD.code_digest IS DISTINCT FROM NEW.code_digest
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'recovery code identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.used_at IS NOT NULL AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'used recovery code is immutable' USING ERRCODE = '55000';
  END IF;
  IF (NEW.used_at IS NULL) <> (NEW.used_request_id IS NULL) THEN
    RAISE EXCEPTION 'recovery use requires time and request identity' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER auth_recovery_codes_guard
    BEFORE UPDATE OR DELETE ON auth_recovery_codes FOR EACH ROW
    EXECUTE FUNCTION guard_auth_recovery_code();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TRIGGER security_audit_events_immutable
    BEFORE UPDATE OR DELETE ON security_audit_events FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TRIGGER storage_reconciliation_items_immutable
    BEFORE UPDATE OR DELETE ON storage_reconciliation_items FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
