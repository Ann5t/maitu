CREATE TABLE IF NOT EXISTS goal_context_entries (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  origin_goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  origin_session_id uuid REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  source_kind text NOT NULL CHECK (source_kind IN (
    'contract', 'contribution', 'evidence', 'artifact', 'input_artifact',
    'review_decision', 'tool_call', 'environment'
  )),
  source_record_id uuid NOT NULL,
  source_fragment text,
  title text NOT NULL CHECK (btrim(title) <> ''),
  content_hash text NOT NULL CHECK (content_hash ~ '^sha256:[0-9a-f]{64}$'),
  importance text NOT NULL DEFAULT 'normal' CHECK (importance IN (
    'essential', 'high', 'normal', 'archive'
  )),
  untrusted_content boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS goal_context_entry_source_idx
  ON goal_context_entries (
    project_id, origin_goal_branch_id, source_kind, source_record_id,
    COALESCE(source_fragment, '')
  );
CREATE INDEX IF NOT EXISTS goal_context_entry_branch_idx
  ON goal_context_entries (origin_goal_branch_id, created_at, id);

CREATE TABLE IF NOT EXISTS goal_context_snapshots (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  version integer NOT NULL CHECK (version > 0),
  parent_snapshot_id uuid REFERENCES goal_context_snapshots(id) ON DELETE RESTRICT,
  inherited_from_session_id uuid REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  contract_version_id uuid NOT NULL REFERENCES goal_contract_versions(id) ON DELETE RESTRICT,
  environment_manifest_id uuid REFERENCES environment_manifests(id) ON DELETE RESTRICT,
  required_context jsonb NOT NULL CHECK (
    jsonb_typeof(required_context) = 'object'
    AND required_context @> '{"nonFoldable":true}'::jsonb
    AND required_context ? 'contract'
    AND required_context ? 'ancestorContracts'
    AND required_context ? 'permissions'
    AND required_context ? 'creationState'
    AND required_context ? 'unresolvedAttention'
  ),
  required_context_hash text NOT NULL CHECK (required_context_hash ~ '^sha256:[0-9a-f]{64}$'),
  budget_policy jsonb NOT NULL CHECK (
    jsonb_typeof(budget_policy) = 'object'
    AND budget_policy @> '{"requiredContextUnabridged":true}'::jsonb
  ),
  catalog_hash text NOT NULL CHECK (catalog_hash ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (session_id, version),
  CHECK (
    (parent_snapshot_id IS NULL AND inherited_from_session_id IS NULL)
    OR (parent_snapshot_id IS NOT NULL AND inherited_from_session_id IS NOT NULL)
  )
);

ALTER TABLE goal_sessions
  ADD COLUMN IF NOT EXISTS context_snapshot_id uuid;

DO $$ BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'goal_sessions_context_snapshot_fk'
      AND conrelid = 'goal_sessions'::regclass
  ) THEN
    ALTER TABLE goal_sessions
      ADD CONSTRAINT goal_sessions_context_snapshot_fk
      FOREIGN KEY (context_snapshot_id) REFERENCES goal_context_snapshots(id)
      ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS goal_context_snapshot_entries (
  snapshot_id uuid NOT NULL REFERENCES goal_context_snapshots(id) ON DELETE CASCADE,
  entry_id uuid NOT NULL REFERENCES goal_context_entries(id) ON DELETE RESTRICT,
  inheritance_kind text NOT NULL CHECK (inheritance_kind IN (
    'required', 'inherited', 'local', 'integrated'
  )),
  rank integer NOT NULL CHECK (rank >= 0),
  inclusion_reason text NOT NULL CHECK (btrim(inclusion_reason) <> ''),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (snapshot_id, entry_id)
);

CREATE TABLE IF NOT EXISTS goal_context_derivations (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  entry_id uuid NOT NULL REFERENCES goal_context_entries(id) ON DELETE RESTRICT,
  kind text NOT NULL CHECK (kind IN (
    'summary', 'fulltext_index', 'retrieval_index'
  )),
  generation integer NOT NULL CHECK (generation > 0),
  generator text NOT NULL,
  source_hash text NOT NULL CHECK (source_hash ~ '^sha256:[0-9a-f]{64}$'),
  payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
  content_hash text NOT NULL CHECK (content_hash ~ '^sha256:[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (entry_id, kind, generation)
);

CREATE INDEX IF NOT EXISTS goal_context_derivation_latest_idx
  ON goal_context_derivations (entry_id, kind, generation DESC);

CREATE TABLE IF NOT EXISTS goal_context_reads (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  goal_branch_id uuid NOT NULL REFERENCES goal_branches(id) ON DELETE RESTRICT,
  session_id uuid NOT NULL REFERENCES goal_sessions(id) ON DELETE RESTRICT,
  snapshot_id uuid NOT NULL REFERENCES goal_context_snapshots(id) ON DELETE RESTRICT,
  entry_id uuid REFERENCES goal_context_entries(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  disclosure_level text NOT NULL CHECK (disclosure_level IN (
    'required_envelope', 'catalog', 'summary', 'snippet', 'full'
  )),
  purpose text NOT NULL CHECK (btrim(purpose) <> ''),
  query text,
  source_hash text NOT NULL CHECK (source_hash ~ '^sha256:[0-9a-f]{64}$'),
  result_hash text NOT NULL CHECK (result_hash ~ '^sha256:[0-9a-f]{64}$'),
  result_chars integer NOT NULL CHECK (result_chars >= 0),
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, client_request_id),
  CHECK (
    (entry_id IS NULL AND disclosure_level IN ('required_envelope', 'catalog'))
    OR (entry_id IS NOT NULL AND disclosure_level IN ('summary', 'snippet', 'full'))
  )
);

CREATE INDEX IF NOT EXISTS goal_context_reads_session_idx
  ON goal_context_reads (session_id, created_at DESC, id);

CREATE TABLE IF NOT EXISTS goal_context_edges (
  project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  from_entry_id uuid NOT NULL REFERENCES goal_context_entries(id) ON DELETE RESTRICT,
  to_entry_id uuid NOT NULL REFERENCES goal_context_entries(id) ON DELETE RESTRICT,
  relation text NOT NULL CHECK (relation IN (
    'supports', 'refutes', 'blocks', 'derived_from', 'supersedes',
    'produced_with', 'integrated_from'
  )),
  explanation text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (from_entry_id, to_entry_id, relation),
  CHECK (from_entry_id <> to_entry_id)
);

CREATE OR REPLACE FUNCTION goal_validate_context_entry_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  session_project uuid;
  session_branch uuid;
  source_project uuid;
  source_branch uuid;
  source_session uuid;
BEGIN
  SELECT project_id INTO branch_project
  FROM goal_branches WHERE id = NEW.origin_goal_branch_id;
  IF branch_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'ContextEntry origin branch crosses project scope'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.origin_session_id IS NOT NULL THEN
    SELECT project_id, goal_branch_id INTO session_project, session_branch
    FROM goal_sessions WHERE id = NEW.origin_session_id;
    IF session_project IS DISTINCT FROM NEW.project_id
       OR session_branch IS DISTINCT FROM NEW.origin_goal_branch_id THEN
      RAISE EXCEPTION 'ContextEntry origin Session crosses branch scope'
        USING ERRCODE = '23514';
    END IF;
  END IF;

  CASE NEW.source_kind
    WHEN 'contract' THEN
      SELECT project_id, goal_branch_id INTO source_project, source_branch
      FROM goal_contract_versions WHERE id = NEW.source_record_id;
    WHEN 'contribution' THEN
      SELECT project_id, goal_branch_id, session_id
        INTO source_project, source_branch, source_session
      FROM goal_contributions WHERE id = NEW.source_record_id;
    WHEN 'evidence' THEN
      SELECT project_id, goal_branch_id, session_id
        INTO source_project, source_branch, source_session
      FROM goal_evidence WHERE id = NEW.source_record_id;
    WHEN 'artifact' THEN
      SELECT project_id INTO source_project
      FROM artifacts WHERE id = NEW.source_record_id;
      source_branch := NEW.origin_goal_branch_id;
    WHEN 'input_artifact' THEN
      SELECT project_id, goal_branch_id, session_id
        INTO source_project, source_branch, source_session
      FROM input_artifacts WHERE id = NEW.source_record_id;
    WHEN 'review_decision' THEN
      SELECT d.project_id, g.goal_branch_id, g.session_id
        INTO source_project, source_branch, source_session
      FROM goal_review_decisions d
      JOIN goal_review_gates g ON g.id = d.review_gate_id
      WHERE d.id = NEW.source_record_id;
    WHEN 'tool_call' THEN
      SELECT project_id, goal_branch_id, session_id
        INTO source_project, source_branch, source_session
      FROM tool_calls WHERE id = NEW.source_record_id;
    WHEN 'environment' THEN
      SELECT b.project_id, b.goal_branch_id, b.session_id
        INTO source_project, source_branch, source_session
      FROM session_environment_bindings b
      WHERE b.environment_manifest_id = NEW.source_record_id
        AND b.session_id = NEW.origin_session_id;
  END CASE;

  IF source_project IS DISTINCT FROM NEW.project_id
     OR source_branch IS DISTINCT FROM NEW.origin_goal_branch_id
     OR (source_session IS NOT NULL
       AND NEW.origin_session_id IS DISTINCT FROM source_session) THEN
    RAISE EXCEPTION 'ContextEntry source crosses aggregate scope or does not exist'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_entries_scope
    AFTER INSERT ON goal_context_entries
    FOR EACH ROW EXECUTE FUNCTION goal_validate_context_entry_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_context_snapshot_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  branch_project uuid;
  session_project uuid;
  session_branch uuid;
  contract_project uuid;
  contract_branch uuid;
  parent_project uuid;
  parent_session uuid;
  inherited_project uuid;
  environment_fingerprint text;
  session_fingerprint text;
BEGIN
  SELECT project_id INTO branch_project
  FROM goal_branches WHERE id = NEW.goal_branch_id;
  SELECT session.project_id, session.goal_branch_id, session.environment_fingerprint
    INTO session_project, session_branch, session_fingerprint
  FROM goal_sessions session WHERE session.id = NEW.session_id;
  SELECT project_id, goal_branch_id INTO contract_project, contract_branch
  FROM goal_contract_versions WHERE id = NEW.contract_version_id;
  IF branch_project IS DISTINCT FROM NEW.project_id
     OR session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id
     OR contract_project IS DISTINCT FROM NEW.project_id
     OR contract_branch IS DISTINCT FROM NEW.goal_branch_id THEN
    RAISE EXCEPTION 'ContextSnapshot crosses project or branch scope'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.parent_snapshot_id IS NOT NULL THEN
    SELECT project_id, session_id INTO parent_project, parent_session
    FROM goal_context_snapshots WHERE id = NEW.parent_snapshot_id;
    SELECT project_id INTO inherited_project
    FROM goal_sessions WHERE id = NEW.inherited_from_session_id;
    IF parent_project IS DISTINCT FROM NEW.project_id
       OR inherited_project IS DISTINCT FROM NEW.project_id
       OR parent_session IS DISTINCT FROM NEW.inherited_from_session_id THEN
      RAISE EXCEPTION 'ContextSnapshot parent inheritance point is inconsistent'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NEW.environment_manifest_id IS NOT NULL THEN
    SELECT fingerprint INTO environment_fingerprint
    FROM environment_manifests WHERE id = NEW.environment_manifest_id;
    IF environment_fingerprint IS DISTINCT FROM session_fingerprint THEN
      RAISE EXCEPTION 'ContextSnapshot environment differs from Session binding'
        USING ERRCODE = '23514';
    END IF;
  ELSIF session_fingerprint IS NOT NULL THEN
    RAISE EXCEPTION 'ContextSnapshot omitted the bound Session environment'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_snapshots_scope
    AFTER INSERT ON goal_context_snapshots
    FOR EACH ROW EXECUTE FUNCTION goal_validate_context_snapshot_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_session_context_pointer()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  snapshot_project uuid;
  snapshot_branch uuid;
  snapshot_session uuid;
BEGIN
  IF NEW.context_snapshot_id IS NULL THEN
    RETURN NEW;
  END IF;
  SELECT project_id, goal_branch_id, session_id
    INTO snapshot_project, snapshot_branch, snapshot_session
  FROM goal_context_snapshots WHERE id = NEW.context_snapshot_id;
  IF snapshot_project IS DISTINCT FROM NEW.project_id
     OR snapshot_branch IS DISTINCT FROM NEW.goal_branch_id
     OR snapshot_session IS DISTINCT FROM NEW.id THEN
    RAISE EXCEPTION 'Session current context pointer crosses aggregate scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_sessions_context_pointer_scope
    AFTER INSERT OR UPDATE OF context_snapshot_id ON goal_sessions
    FOR EACH ROW EXECUTE FUNCTION goal_validate_session_context_pointer();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_snapshot_entry_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  snapshot_project uuid;
  entry_project uuid;
BEGIN
  SELECT project_id INTO snapshot_project
  FROM goal_context_snapshots WHERE id = NEW.snapshot_id;
  SELECT project_id INTO entry_project
  FROM goal_context_entries WHERE id = NEW.entry_id;
  IF snapshot_project IS DISTINCT FROM entry_project THEN
    RAISE EXCEPTION 'ContextSnapshot entry crosses project scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_snapshot_entries_scope
    AFTER INSERT ON goal_context_snapshot_entries
    FOR EACH ROW EXECUTE FUNCTION goal_validate_snapshot_entry_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_context_derivation_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  entry_project uuid;
  entry_hash text;
BEGIN
  SELECT project_id, content_hash INTO entry_project, entry_hash
  FROM goal_context_entries WHERE id = NEW.entry_id;
  IF entry_project IS DISTINCT FROM NEW.project_id
     OR entry_hash IS DISTINCT FROM NEW.source_hash THEN
    RAISE EXCEPTION 'Context derivation is not bound to the exact source version'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_derivations_scope
    AFTER INSERT ON goal_context_derivations
    FOR EACH ROW EXECUTE FUNCTION goal_validate_context_derivation_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_context_read_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  session_project uuid;
  session_branch uuid;
  session_snapshot uuid;
  snapshot_project uuid;
  snapshot_session uuid;
  entry_project uuid;
  entry_hash text;
BEGIN
  SELECT project_id, goal_branch_id, context_snapshot_id
    INTO session_project, session_branch, session_snapshot
  FROM goal_sessions WHERE id = NEW.session_id;
  SELECT project_id, session_id INTO snapshot_project, snapshot_session
  FROM goal_context_snapshots WHERE id = NEW.snapshot_id;
  IF session_project IS DISTINCT FROM NEW.project_id
     OR session_branch IS DISTINCT FROM NEW.goal_branch_id
     OR snapshot_project IS DISTINCT FROM NEW.project_id
     OR snapshot_session IS DISTINCT FROM NEW.session_id
     OR session_snapshot IS DISTINCT FROM NEW.snapshot_id THEN
    RAISE EXCEPTION 'Context read is not bound to the current Session snapshot'
      USING ERRCODE = '23514';
  END IF;
  IF NEW.entry_id IS NOT NULL THEN
    SELECT project_id, content_hash INTO entry_project, entry_hash
    FROM goal_context_entries WHERE id = NEW.entry_id;
    IF entry_project IS DISTINCT FROM NEW.project_id
       OR entry_hash IS DISTINCT FROM NEW.source_hash
       OR NOT EXISTS (
         SELECT 1 FROM goal_context_snapshot_entries membership
         WHERE membership.snapshot_id = NEW.snapshot_id
           AND membership.entry_id = NEW.entry_id
       ) THEN
      RAISE EXCEPTION 'Context read entry is outside the inherited catalog'
        USING ERRCODE = '23514';
    END IF;
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_reads_scope
    AFTER INSERT ON goal_context_reads
    FOR EACH ROW EXECUTE FUNCTION goal_validate_context_read_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION goal_validate_context_edge_scope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  from_project uuid;
  to_project uuid;
BEGIN
  SELECT project_id INTO from_project
  FROM goal_context_entries WHERE id = NEW.from_entry_id;
  SELECT project_id INTO to_project
  FROM goal_context_entries WHERE id = NEW.to_entry_id;
  IF from_project IS DISTINCT FROM NEW.project_id
     OR to_project IS DISTINCT FROM NEW.project_id THEN
    RAISE EXCEPTION 'Context provenance edge crosses project scope'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

DO $$ BEGIN
  CREATE TRIGGER goal_context_edges_scope
    AFTER INSERT ON goal_context_edges
    FOR EACH ROW EXECUTE FUNCTION goal_validate_context_edge_scope();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'goal_context_entries',
    'goal_context_snapshots',
    'goal_context_snapshot_entries',
    'goal_context_derivations',
    'goal_context_reads',
    'goal_context_edges'
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
