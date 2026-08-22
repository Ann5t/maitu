ALTER TABLE goal_attention_items
  DROP CONSTRAINT IF EXISTS goal_attention_items_kind_check;
ALTER TABLE goal_attention_items
  ADD CONSTRAINT goal_attention_items_kind_check CHECK (kind IN (
    'branch_review', 'dependency', 'judgment', 'merge_review', 'exception',
    'manual_pause', 'continuation_required', 'child_result_ready', 'contract_review',
    'action_run_failure'
  ));

ALTER TABLE goal_events
  DROP CONSTRAINT IF EXISTS goal_events_aggregate_type_check;
ALTER TABLE goal_events
  ADD CONSTRAINT goal_events_aggregate_type_check CHECK (aggregate_type IN (
    'proposal', 'goal_branch', 'session', 'contract', 'contract_revision',
    'evidence', 'review_gate', 'integration', 'attention', 'tool',
    'input_artifact', 'project', 'action_run', 'notification'
  ));

CREATE TABLE IF NOT EXISTS scheduler_workers (
  id uuid PRIMARY KEY,
  client_request_id uuid NOT NULL UNIQUE,
  display_name text NOT NULL CHECK (display_name <> ''),
  token_digest text NOT NULL CHECK (token_digest ~ '^sha256:[0-9a-f]{64}$'),
  capabilities jsonb NOT NULL CHECK (
    jsonb_typeof(capabilities) = 'array' AND jsonb_array_length(capabilities) > 0
  ),
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'draining', 'revoked')),
  registered_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz,
  revocation_reason text,
  CHECK (
    (status IN ('active', 'draining') AND revoked_at IS NULL AND revocation_reason IS NULL)
    OR (status = 'revoked' AND revoked_at IS NOT NULL AND revocation_reason <> '')
  )
);

CREATE TABLE IF NOT EXISTS goal_action_runs (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  kind text NOT NULL CHECK (kind IN (
    'agent_step', 'runner_job', 'tool_lease', 'review', 'integration', 'maintenance'
  )),
  capability text NOT NULL CHECK (
    capability ~ '^[a-z][a-z0-9._-]{0,159}$'
  ),
  subject_kind text NOT NULL CHECK (subject_kind IN (
    'none', 'runner_job', 'tool_lease', 'review_gate', 'integration'
  )),
  subject_id uuid,
  payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
  retry_safety text NOT NULL CHECK (retry_safety IN ('safe', 'unsafe', 'unknown')),
  status text NOT NULL DEFAULT 'queued' CHECK (status IN (
    'queued', 'running', 'waiting', 'cancellation_requested',
    'succeeded', 'failed', 'cancelled'
  )),
  attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 10),
  fencing_counter bigint NOT NULL DEFAULT 0 CHECK (fencing_counter >= 0),
  available_at timestamptz NOT NULL DEFAULT now(),
  deadline_at timestamptz,
  last_error_code text,
  last_error_summary text,
  result jsonb CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  started_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  UNIQUE (project_id, client_request_id),
  CHECK (
    (subject_kind = 'none' AND subject_id IS NULL)
    OR (subject_kind <> 'none' AND subject_id IS NOT NULL)
  ),
  CHECK (deadline_at IS NULL OR deadline_at > created_at),
  CHECK (attempt_count <= max_attempts),
  CHECK (
    (status IN ('succeeded', 'failed', 'cancelled') AND completed_at IS NOT NULL)
    OR (status NOT IN ('succeeded', 'failed', 'cancelled') AND completed_at IS NULL)
  )
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_action_runs_subject_idx
  ON goal_action_runs (subject_kind, subject_id) WHERE subject_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS goal_action_runs_queue_idx
  ON goal_action_runs (capability, available_at, created_at)
  WHERE status = 'queued';
CREATE INDEX IF NOT EXISTS goal_action_runs_session_idx
  ON goal_action_runs (session_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS action_run_leases (
  id uuid PRIMARY KEY,
  action_run_id uuid NOT NULL REFERENCES goal_action_runs(id) ON DELETE RESTRICT,
  worker_id uuid NOT NULL REFERENCES scheduler_workers(id) ON DELETE RESTRICT,
  claim_request_id uuid NOT NULL,
  attempt_number integer NOT NULL CHECK (attempt_number > 0),
  fencing_token bigint NOT NULL CHECK (fencing_token > 0),
  renewal_token_digest text NOT NULL CHECK (
    renewal_token_digest ~ '^sha256:[0-9a-f]{64}$'
  ),
  status text NOT NULL DEFAULT 'active' CHECK (status IN (
    'active', 'succeeded', 'failed', 'expired', 'cancelled'
  )),
  acquired_at timestamptz NOT NULL DEFAULT now(),
  last_heartbeat_at timestamptz NOT NULL DEFAULT now(),
  soft_expires_at timestamptz NOT NULL,
  hard_expires_at timestamptz NOT NULL,
  outcome jsonb CHECK (outcome IS NULL OR jsonb_typeof(outcome) = 'object'),
  error_code text,
  error_summary text,
  completed_at timestamptz,
  UNIQUE (worker_id, claim_request_id),
  UNIQUE (action_run_id, attempt_number),
  UNIQUE (action_run_id, fencing_token),
  CHECK (soft_expires_at <= hard_expires_at),
  CHECK (
    (status = 'active' AND completed_at IS NULL)
    OR (status <> 'active' AND completed_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX IF NOT EXISTS action_run_leases_one_active_action_idx
  ON action_run_leases (action_run_id) WHERE status = 'active';
CREATE UNIQUE INDEX IF NOT EXISTS action_run_leases_one_active_worker_idx
  ON action_run_leases (worker_id) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS action_run_leases_expiry_idx
  ON action_run_leases (soft_expires_at, hard_expires_at) WHERE status = 'active';

CREATE TABLE IF NOT EXISTS goal_action_events (
  id uuid PRIMARY KEY,
  sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  action_run_id uuid NOT NULL REFERENCES goal_action_runs(id) ON DELETE RESTRICT,
  lease_id uuid REFERENCES action_run_leases(id) ON DELETE RESTRICT,
  event_type text NOT NULL CHECK (event_type <> ''),
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'worker', 'system')),
  actor_identity text,
  detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS goal_action_events_action_idx
  ON goal_action_events (action_run_id, sequence);

CREATE TABLE IF NOT EXISTS goal_notifications (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  action_run_id uuid REFERENCES goal_action_runs(id) ON DELETE RESTRICT,
  attention_item_id uuid REFERENCES goal_attention_items(id) ON DELETE RESTRICT,
  dedupe_key text NOT NULL,
  kind text NOT NULL CHECK (kind IN (
    'action_waiting', 'action_failed', 'action_timed_out', 'tool_lease_expired'
  )),
  severity text NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
  title text NOT NULL CHECK (title <> ''),
  summary text NOT NULL CHECK (summary <> ''),
  status text NOT NULL DEFAULT 'unread' CHECK (status IN ('unread', 'read', 'resolved')),
  created_at timestamptz NOT NULL DEFAULT now(),
  read_at timestamptz,
  resolved_at timestamptz,
  CHECK (
    (status = 'unread' AND read_at IS NULL AND resolved_at IS NULL)
    OR (status = 'read' AND read_at IS NOT NULL AND resolved_at IS NULL)
    OR (status = 'resolved' AND resolved_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_notifications_open_dedupe_idx
  ON goal_notifications (project_id, dedupe_key) WHERE status <> 'resolved';
CREATE INDEX IF NOT EXISTS goal_notifications_project_idx
  ON goal_notifications (project_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS notification_outbox (
  id uuid PRIMARY KEY,
  notification_id uuid NOT NULL REFERENCES goal_notifications(id) ON DELETE RESTRICT,
  adapter text NOT NULL CHECK (adapter <> ''),
  dedupe_key text NOT NULL UNIQUE,
  payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
  payload_digest text NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
  status text NOT NULL CHECK (status IN ('pending', 'suppressed', 'sent', 'failed')),
  attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  available_at timestamptz NOT NULL DEFAULT now(),
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  CHECK (
    (status = 'pending' AND completed_at IS NULL)
    OR (status <> 'pending' AND completed_at IS NOT NULL)
  )
);

ALTER TABLE tool_leases
  ADD COLUMN IF NOT EXISTS client_request_id uuid,
  ADD COLUMN IF NOT EXISTS request_hash text,
  ADD COLUMN IF NOT EXISTS plugin_installation_id uuid,
  ADD COLUMN IF NOT EXISTS input jsonb NOT NULL DEFAULT '{}'::jsonb,
  ADD COLUMN IF NOT EXISTS runtime_image_digest text,
  ADD COLUMN IF NOT EXISTS runtime_entry_digest text,
  ADD COLUMN IF NOT EXISTS runner_digest text,
  ADD COLUMN IF NOT EXISTS cleanup_status text NOT NULL DEFAULT 'not_required',
  ADD COLUMN IF NOT EXISTS last_error_code text,
  ADD COLUMN IF NOT EXISTS last_error_summary text;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_installation_fk
    FOREIGN KEY (plugin_installation_id) REFERENCES plugin_installations(id) ON DELETE RESTRICT;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_request_hash_check
    CHECK (request_hash IS NULL OR request_hash ~ '^sha256:[0-9a-f]{64}$');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_runtime_image_check
    CHECK (runtime_image_digest IS NULL OR runtime_image_digest ~ '^sha256:[0-9a-f]{64}$');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_runtime_entry_check
    CHECK (runtime_entry_digest IS NULL OR runtime_entry_digest ~ '^sha256:[0-9a-f]{64}$');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_runner_digest_check
    CHECK (runner_digest IS NULL OR runner_digest ~ '^sha256:[0-9a-f]{64}$');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_cleanup_status_check
    CHECK (cleanup_status IN ('not_required', 'pending', 'running', 'succeeded', 'failed'));
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE tool_leases
    ADD CONSTRAINT tool_leases_v2_complete_identity_check
    CHECK (
      client_request_id IS NULL OR (
        request_hash IS NOT NULL
        AND plugin_installation_id IS NOT NULL
        AND runtime_image_digest IS NOT NULL
        AND runtime_entry_digest IS NOT NULL
        AND runner_digest IS NOT NULL
      )
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS tool_leases_client_request_idx
  ON tool_leases (project_id, client_request_id) WHERE client_request_id IS NOT NULL;

CREATE OR REPLACE FUNCTION goal_validate_action_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  subject_project uuid;
  subject_branch uuid;
  subject_session uuid;
BEGIN
  SELECT project_id, goal_branch_id INTO session_project, session_branch
  FROM goal_sessions WHERE id = NEW.session_id;
  IF session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'ActionRun Session crosses aggregate scope' USING ERRCODE = '23514';
  END IF;
  IF NEW.subject_kind = 'runner_job' THEN
    SELECT project_id, goal_branch_id, session_id
    INTO subject_project, subject_branch, subject_session
    FROM runner_jobs WHERE id = NEW.subject_id;
  ELSIF NEW.subject_kind = 'tool_lease' THEN
    SELECT project_id, goal_branch_id, session_id
    INTO subject_project, subject_branch, subject_session
    FROM tool_leases WHERE id = NEW.subject_id;
  ELSIF NEW.subject_kind = 'review_gate' THEN
    SELECT project_id, goal_branch_id, session_id
    INTO subject_project, subject_branch, subject_session
    FROM goal_review_gates WHERE id = NEW.subject_id;
  ELSIF NEW.subject_kind = 'integration' THEN
    SELECT project_id, source_goal_branch_id, NULL::uuid
    INTO subject_project, subject_branch, subject_session
    FROM goal_integrations WHERE id = NEW.subject_id;
  END IF;
  IF NEW.subject_kind <> 'none' AND (
       subject_project IS DISTINCT FROM NEW.project_id
       OR subject_branch IS DISTINCT FROM NEW.goal_branch_id
       OR (subject_session IS NOT NULL AND subject_session IS DISTINCT FROM NEW.session_id)
     ) THEN
    RAISE EXCEPTION 'ActionRun subject crosses aggregate scope or does not exist'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_action_runs_scope
    BEFORE INSERT ON goal_action_runs FOR EACH ROW
    EXECUTE FUNCTION goal_validate_action_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_scheduler_worker()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'SchedulerWorker audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.client_request_id IS DISTINCT FROM NEW.client_request_id
     OR OLD.display_name IS DISTINCT FROM NEW.display_name
     OR OLD.token_digest IS DISTINCT FROM NEW.token_digest
     OR OLD.capabilities IS DISTINCT FROM NEW.capabilities
     OR OLD.registered_at IS DISTINCT FROM NEW.registered_at THEN
    RAISE EXCEPTION 'SchedulerWorker identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'revoked' AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'revoked SchedulerWorker is immutable' USING ERRCODE = '55000';
  END IF;
  IF (OLD.status, NEW.status) NOT IN (
    ('active', 'active'), ('active', 'draining'), ('active', 'revoked'),
    ('draining', 'draining'), ('draining', 'active'), ('draining', 'revoked')
  ) THEN
    RAISE EXCEPTION 'invalid SchedulerWorker transition' USING ERRCODE = '23514';
  END IF;
  IF NEW.last_seen_at < OLD.last_seen_at THEN
    RAISE EXCEPTION 'SchedulerWorker last_seen_at cannot move backwards' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER scheduler_workers_guard
    BEFORE UPDATE OR DELETE ON scheduler_workers FOR EACH ROW
    EXECUTE FUNCTION goal_guard_scheduler_worker();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_action_run()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'ActionRun audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.client_request_id IS DISTINCT FROM NEW.client_request_id
     OR OLD.request_hash IS DISTINCT FROM NEW.request_hash
     OR OLD.kind IS DISTINCT FROM NEW.kind
     OR OLD.capability IS DISTINCT FROM NEW.capability
     OR OLD.subject_kind IS DISTINCT FROM NEW.subject_kind
     OR OLD.subject_id IS DISTINCT FROM NEW.subject_id
     OR OLD.payload IS DISTINCT FROM NEW.payload
     OR OLD.retry_safety IS DISTINCT FROM NEW.retry_safety
     OR OLD.max_attempts IS DISTINCT FROM NEW.max_attempts
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'ActionRun request identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status IN ('succeeded', 'failed', 'cancelled') AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal ActionRun is immutable' USING ERRCODE = '55000';
  END IF;
  IF (OLD.status, NEW.status) NOT IN (
    ('queued', 'queued'), ('queued', 'running'), ('queued', 'failed'), ('queued', 'cancelled'),
    ('running', 'running'), ('running', 'queued'), ('running', 'waiting'),
    ('running', 'cancellation_requested'), ('running', 'succeeded'),
    ('running', 'failed'), ('running', 'cancelled'),
    ('cancellation_requested', 'cancellation_requested'),
    ('cancellation_requested', 'succeeded'), ('cancellation_requested', 'cancelled'),
    ('cancellation_requested', 'failed'),
    ('waiting', 'waiting'), ('waiting', 'queued'), ('waiting', 'failed'),
    ('waiting', 'cancelled')
  ) THEN
    RAISE EXCEPTION 'invalid ActionRun transition' USING ERRCODE = '23514';
  END IF;
  IF NEW.attempt_count < OLD.attempt_count
     OR NEW.attempt_count > OLD.attempt_count + 1
     OR NEW.fencing_counter < OLD.fencing_counter
     OR NEW.fencing_counter > OLD.fencing_counter + 1 THEN
    RAISE EXCEPTION 'ActionRun counters must be monotonic by one' USING ERRCODE = '23514';
  END IF;
  IF NEW.status = 'running' AND OLD.status = 'queued' AND (
       NEW.attempt_count <> OLD.attempt_count + 1
       OR NEW.fencing_counter <> OLD.fencing_counter + 1
     ) THEN
    RAISE EXCEPTION 'claim must advance ActionRun attempt and fencing counters'
      USING ERRCODE = '23514';
  END IF;
  IF NOT (NEW.status = 'running' AND OLD.status = 'queued') AND (
       NEW.attempt_count <> OLD.attempt_count
       OR NEW.fencing_counter <> OLD.fencing_counter
     ) THEN
    RAISE EXCEPTION 'only claim may advance ActionRun counters' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_action_runs_guard
    BEFORE UPDATE OR DELETE ON goal_action_runs FOR EACH ROW
    EXECUTE FUNCTION goal_guard_action_run();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_action_lease()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'ActionLease audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.action_run_id IS DISTINCT FROM NEW.action_run_id
     OR OLD.worker_id IS DISTINCT FROM NEW.worker_id
     OR OLD.claim_request_id IS DISTINCT FROM NEW.claim_request_id
     OR OLD.attempt_number IS DISTINCT FROM NEW.attempt_number
     OR OLD.fencing_token IS DISTINCT FROM NEW.fencing_token
     OR OLD.renewal_token_digest IS DISTINCT FROM NEW.renewal_token_digest
     OR OLD.acquired_at IS DISTINCT FROM NEW.acquired_at
     OR OLD.hard_expires_at IS DISTINCT FROM NEW.hard_expires_at THEN
    RAISE EXCEPTION 'ActionLease identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status <> 'active' AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal ActionLease is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'active' AND NEW.status NOT IN (
    'active', 'succeeded', 'failed', 'expired', 'cancelled'
  ) THEN
    RAISE EXCEPTION 'invalid ActionLease transition' USING ERRCODE = '23514';
  END IF;
  IF NEW.last_heartbeat_at < OLD.last_heartbeat_at
     OR NEW.soft_expires_at < OLD.soft_expires_at
     OR NEW.soft_expires_at > NEW.hard_expires_at THEN
    RAISE EXCEPTION 'ActionLease heartbeat/expiry cannot move backwards or exceed hard expiry'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER action_run_leases_guard
    BEFORE UPDATE OR DELETE ON action_run_leases FOR EACH ROW
    EXECUTE FUNCTION goal_guard_action_lease();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_action_lease_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  action_attempt integer;
  action_fencing bigint;
BEGIN
  SELECT attempt_count, fencing_counter INTO action_attempt, action_fencing
  FROM goal_action_runs WHERE id = NEW.action_run_id;
  IF action_attempt IS DISTINCT FROM NEW.attempt_number
     OR action_fencing IS DISTINCT FROM NEW.fencing_token THEN
    RAISE EXCEPTION 'ActionLease does not match current ActionRun fencing state'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE CONSTRAINT TRIGGER action_run_leases_scope
    AFTER INSERT ON action_run_leases DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION goal_validate_action_lease_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_action_child_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  action_project uuid;
BEGIN
  SELECT project_id INTO action_project FROM goal_action_runs WHERE id = NEW.action_run_id;
  IF action_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'Action child record crosses project scope' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_action_events_scope
    BEFORE INSERT ON goal_action_events FOR EACH ROW
    EXECUTE FUNCTION goal_validate_action_child_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_notification_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  action_project uuid;
BEGIN
  IF NEW.session_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO session_project, session_branch
    FROM goal_sessions WHERE id = NEW.session_id;
    IF session_project IS DISTINCT FROM NEW.project_id
       OR (NEW.goal_branch_id IS NOT NULL AND session_branch IS DISTINCT FROM NEW.goal_branch_id) THEN
      RAISE EXCEPTION 'Notification Session crosses aggregate scope' USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NEW.action_run_id IS NOT NULL THEN
    SELECT project_id INTO action_project FROM goal_action_runs WHERE id = NEW.action_run_id;
    IF action_project IS DISTINCT FROM NEW.project_id THEN
      RAISE EXCEPTION 'Notification ActionRun crosses aggregate scope' USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_notifications_scope
    BEFORE INSERT ON goal_notifications FOR EACH ROW
    EXECUTE FUNCTION goal_validate_notification_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_notification()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'Notification audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.action_run_id IS DISTINCT FROM NEW.action_run_id
     OR OLD.attention_item_id IS DISTINCT FROM NEW.attention_item_id
     OR OLD.dedupe_key IS DISTINCT FROM NEW.dedupe_key
     OR OLD.kind IS DISTINCT FROM NEW.kind
     OR OLD.severity IS DISTINCT FROM NEW.severity
     OR OLD.title IS DISTINCT FROM NEW.title
     OR OLD.summary IS DISTINCT FROM NEW.summary
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'Notification identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'resolved' AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'resolved Notification is immutable' USING ERRCODE = '55000';
  END IF;
  IF (OLD.status, NEW.status) NOT IN (
    ('unread', 'unread'), ('unread', 'read'), ('unread', 'resolved'),
    ('read', 'read'), ('read', 'resolved')
  ) THEN
    RAISE EXCEPTION 'invalid Notification transition' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_notifications_guard
    BEFORE UPDATE OR DELETE ON goal_notifications FOR EACH ROW
    EXECUTE FUNCTION goal_guard_notification();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_tool_lease()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'ToolLease audit record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.project_id IS DISTINCT FROM NEW.project_id
     OR OLD.goal_branch_id IS DISTINCT FROM NEW.goal_branch_id
     OR OLD.session_id IS DISTINCT FROM NEW.session_id
     OR OLD.plugin_id IS DISTINCT FROM NEW.plugin_id
     OR OLD.plugin_version IS DISTINCT FROM NEW.plugin_version
     OR OLD.plugin_digest IS DISTINCT FROM NEW.plugin_digest
     OR OLD.tool_name IS DISTINCT FROM NEW.tool_name
     OR OLD.environment_fingerprint IS DISTINCT FROM NEW.environment_fingerprint
     OR OLD.base_workspace_snapshot IS DISTINCT FROM NEW.base_workspace_snapshot
     OR OLD.renewal_token_digest IS DISTINCT FROM NEW.renewal_token_digest
     OR OLD.resource_policy IS DISTINCT FROM NEW.resource_policy
     OR OLD.created_at IS DISTINCT FROM NEW.created_at
     OR OLD.hard_expires_at IS DISTINCT FROM NEW.hard_expires_at
     OR OLD.client_request_id IS DISTINCT FROM NEW.client_request_id
     OR OLD.request_hash IS DISTINCT FROM NEW.request_hash
     OR OLD.plugin_installation_id IS DISTINCT FROM NEW.plugin_installation_id
     OR OLD.input IS DISTINCT FROM NEW.input
     OR OLD.runtime_image_digest IS DISTINCT FROM NEW.runtime_image_digest
     OR OLD.runtime_entry_digest IS DISTINCT FROM NEW.runtime_entry_digest
     OR OLD.runner_digest IS DISTINCT FROM NEW.runner_digest THEN
    RAISE EXCEPTION 'ToolLease identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status IN ('expired', 'released', 'failed', 'cancelled')
     AND OLD.cleanup_status IN ('not_required', 'succeeded', 'failed')
     AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal ToolLease is immutable' USING ERRCODE = '55000';
  END IF;
  IF (OLD.status, NEW.status) NOT IN (
    ('requested', 'requested'), ('requested', 'active'), ('requested', 'failed'),
    ('requested', 'expired'), ('requested', 'cancelled'),
    ('active', 'active'), ('active', 'released'), ('active', 'expired'),
    ('active', 'failed'), ('active', 'cancelled'),
    ('released', 'released'), ('expired', 'expired'), ('failed', 'failed'),
    ('cancelled', 'cancelled')
  ) THEN
    RAISE EXCEPTION 'invalid ToolLease transition' USING ERRCODE = '23514';
  END IF;
  IF NEW.soft_expires_at < OLD.soft_expires_at
     OR NEW.soft_expires_at > NEW.hard_expires_at
     OR (NEW.last_heartbeat_at IS NOT NULL AND OLD.last_heartbeat_at IS NOT NULL
         AND NEW.last_heartbeat_at < OLD.last_heartbeat_at) THEN
    RAISE EXCEPTION 'ToolLease heartbeat/expiry cannot move backwards or exceed hard expiry'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER tool_leases_guard
    BEFORE UPDATE OR DELETE ON tool_leases FOR EACH ROW
    EXECUTE FUNCTION goal_guard_tool_lease();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  CREATE TRIGGER goal_action_events_immutable
    BEFORE UPDATE OR DELETE ON goal_action_events FOR EACH ROW
    EXECUTE FUNCTION goal_reject_immutable_mutation();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_guard_notification_outbox()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'Notification outbox record is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.id IS DISTINCT FROM NEW.id
     OR OLD.notification_id IS DISTINCT FROM NEW.notification_id
     OR OLD.adapter IS DISTINCT FROM NEW.adapter
     OR OLD.dedupe_key IS DISTINCT FROM NEW.dedupe_key
     OR OLD.payload IS DISTINCT FROM NEW.payload
     OR OLD.payload_digest IS DISTINCT FROM NEW.payload_digest
     OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'Notification outbox identity is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status <> 'pending' AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal Notification delivery is immutable' USING ERRCODE = '55000';
  END IF;
  IF OLD.status = 'pending' AND NEW.status NOT IN ('pending', 'suppressed', 'sent', 'failed') THEN
    RAISE EXCEPTION 'invalid Notification delivery transition' USING ERRCODE = '23514';
  END IF;
  IF NEW.attempt_count < OLD.attempt_count THEN
    RAISE EXCEPTION 'Notification attempt_count cannot move backwards' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER notification_outbox_guard
    BEFORE UPDATE OR DELETE ON notification_outbox FOR EACH ROW
    EXECUTE FUNCTION goal_guard_notification_outbox();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
