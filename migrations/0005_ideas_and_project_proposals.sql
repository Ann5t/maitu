-- Ideas are an upstream space.  They remain available after a Project is promoted and every
-- mutable aggregate points at an immutable revision.
CREATE TABLE IF NOT EXISTS ideas (
  id uuid PRIMARY KEY,
  state text NOT NULL DEFAULT 'captured' CHECK (state IN (
    'captured', 'developing', 'proposed', 'promoted', 'archived'
  )),
  current_revision integer NOT NULL DEFAULT 1 CHECK (current_revision > 0),
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  archived_at timestamptz
);

CREATE TABLE IF NOT EXISTS idea_revisions (
  id uuid NOT NULL UNIQUE,
  idea_id uuid NOT NULL REFERENCES ideas(id) ON DELETE CASCADE,
  revision integer NOT NULL CHECK (revision > 0),
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 240),
  body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 20000),
  source_kind text NOT NULL DEFAULT 'text' CHECK (source_kind IN (
    'text', 'file', 'image', 'audio', 'external'
  )),
  source_ref text,
  revision_reason text,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (idea_id, revision)
);

DO $$ BEGIN
  ALTER TABLE ideas
    ADD CONSTRAINT ideas_current_revision_fk
    FOREIGN KEY (id, current_revision)
    REFERENCES idea_revisions(idea_id, revision)
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS idea_links (
  id uuid PRIMARY KEY,
  source_idea_id uuid NOT NULL,
  source_revision integer NOT NULL,
  target_idea_id uuid NOT NULL,
  target_revision integer NOT NULL,
  relation text NOT NULL CHECK (relation IN (
    'related', 'supports', 'contradicts', 'depends_on', 'duplicates'
  )),
  rationale text NOT NULL CHECK (char_length(rationale) BETWEEN 1 AND 4000),
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (source_idea_id <> target_idea_id),
  FOREIGN KEY (source_idea_id, source_revision)
    REFERENCES idea_revisions(idea_id, revision) ON DELETE RESTRICT,
  FOREIGN KEY (target_idea_id, target_revision)
    REFERENCES idea_revisions(idea_id, revision) ON DELETE RESTRICT,
  UNIQUE (source_idea_id, source_revision, target_idea_id, target_revision, relation)
);

CREATE TABLE IF NOT EXISTS project_proposals (
  id uuid PRIMARY KEY,
  status text NOT NULL DEFAULT 'draft' CHECK (status IN (
    'draft', 'awaiting_approval', 'approved', 'rejected', 'cancelled'
  )),
  current_revision integer NOT NULL DEFAULT 1 CHECK (current_revision > 0),
  approved_revision integer CHECK (approved_revision IS NULL OR approved_revision > 0),
  approved_project_id uuid UNIQUE REFERENCES projects(id) ON DELETE RESTRICT,
  approved_root_goal_proposal_id uuid UNIQUE
    REFERENCES goal_branch_proposals(id) ON DELETE RESTRICT,
  decision_rationale text,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  CHECK (
    (status = 'approved' AND approved_revision IS NOT NULL
      AND approved_project_id IS NOT NULL AND approved_root_goal_proposal_id IS NOT NULL)
    OR
    (status <> 'approved' AND approved_project_id IS NULL
      AND approved_root_goal_proposal_id IS NULL)
  )
);

CREATE TABLE IF NOT EXISTS project_proposal_revisions (
  id uuid NOT NULL UNIQUE,
  proposal_id uuid NOT NULL REFERENCES project_proposals(id) ON DELETE CASCADE,
  revision integer NOT NULL CHECK (revision > 0),
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 240),
  project_intent text NOT NULL CHECK (char_length(project_intent) BETWEEN 3 AND 4000),
  why_now text NOT NULL CHECK (char_length(why_now) BETWEEN 1 AND 4000),
  root_goal jsonb NOT NULL CHECK (jsonb_typeof(root_goal) = 'object'),
  retained_notes jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(retained_notes) = 'array'),
  omitted_notes jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(omitted_notes) = 'array'),
  revision_reason text,
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (proposal_id, revision)
);

DO $$ BEGIN
  ALTER TABLE project_proposals
    ADD CONSTRAINT project_proposals_current_revision_fk
    FOREIGN KEY (id, current_revision)
    REFERENCES project_proposal_revisions(proposal_id, revision)
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
  ALTER TABLE project_proposals
    ADD CONSTRAINT project_proposals_approved_revision_fk
    FOREIGN KEY (id, approved_revision)
    REFERENCES project_proposal_revisions(proposal_id, revision)
    DEFERRABLE INITIALLY DEFERRED;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- Source membership belongs to a specific Proposal revision: changing the source set therefore
-- requires a visible new revision instead of silently rewriting provenance.
CREATE TABLE IF NOT EXISTS project_proposal_revision_ideas (
  proposal_id uuid NOT NULL,
  proposal_revision integer NOT NULL,
  idea_id uuid NOT NULL,
  idea_revision integer NOT NULL,
  role text NOT NULL CHECK (role IN ('source', 'supporting', 'constraint', 'omitted')),
  rationale text NOT NULL DEFAULT '',
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (proposal_id, proposal_revision, idea_id),
  FOREIGN KEY (proposal_id, proposal_revision)
    REFERENCES project_proposal_revisions(proposal_id, revision) ON DELETE CASCADE,
  FOREIGN KEY (idea_id, idea_revision)
    REFERENCES idea_revisions(idea_id, revision) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS project_origins (
  project_id uuid PRIMARY KEY REFERENCES projects(id) ON DELETE RESTRICT,
  project_proposal_id uuid NOT NULL UNIQUE REFERENCES project_proposals(id) ON DELETE RESTRICT,
  proposal_revision integer NOT NULL,
  root_goal_proposal_id uuid NOT NULL UNIQUE
    REFERENCES goal_branch_proposals(id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (project_proposal_id, proposal_revision)
    REFERENCES project_proposal_revisions(proposal_id, revision) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS project_origin_ideas (
  project_id uuid NOT NULL REFERENCES project_origins(project_id) ON DELETE RESTRICT,
  idea_id uuid NOT NULL,
  idea_revision integer NOT NULL,
  role text NOT NULL CHECK (role IN ('source', 'supporting', 'constraint', 'omitted')),
  rationale text NOT NULL DEFAULT '',
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, idea_id),
  FOREIGN KEY (idea_id, idea_revision)
    REFERENCES idea_revisions(idea_id, revision) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS idea_command_receipts (
  client_request_id uuid PRIMARY KEY,
  subject_id uuid,
  command_kind text NOT NULL,
  input_hash text NOT NULL CHECK (input_hash ~ '^sha256:[0-9a-f]{64}$'),
  result jsonb NOT NULL CHECK (jsonb_typeof(result) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS idea_events (
  id uuid PRIMARY KEY,
  sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  aggregate_type text NOT NULL CHECK (aggregate_type IN ('idea', 'project_proposal', 'project')),
  aggregate_id uuid NOT NULL,
  event_type text NOT NULL,
  actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'system')),
  client_request_id uuid NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(payload) = 'object'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ideas_updated_idx ON ideas (updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idea_links_source_idx ON idea_links (source_idea_id, created_at);
CREATE INDEX IF NOT EXISTS idea_links_target_idx ON idea_links (target_idea_id, created_at);
CREATE INDEX IF NOT EXISTS project_proposals_status_idx
  ON project_proposals (status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idea_events_aggregate_idx
  ON idea_events (aggregate_type, aggregate_id, sequence);

-- These records are provenance, not editable notes.  Mutable state lives only in the aggregate
-- headers above; all content changes append another revision or event.
DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'idea_revisions',
    'idea_links',
    'project_proposal_revisions',
    'project_proposal_revision_ideas',
    'project_origins',
    'project_origin_ideas',
    'idea_command_receipts',
    'idea_events'
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
